#!/usr/bin/env python3
"""
md98 Windows 98 SE QEMU + QMP + Serial Automation & MCP Server.

Exposes:
  - win98_build_and_audit()
  - win98_run_e2e(fixture_path, wait_ms)
  - win98_send_keys(keys)

Can run as an MCP stdio server (`python3 infra/win98-mcp/server.py`) OR as a CLI tool
for direct agent invocation (`python3 infra/win98-mcp/server.py run-e2e ...`).
"""

import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from PIL import Image

REPO_ROOT = Path(__file__).resolve().parents[2]
QCOW2_PATH = REPO_ROOT / "infra" / "qemu" / "win98se.qcow2"
DIST_DIR = REPO_ROOT / "dist"
FIXTURES_DIR = REPO_ROOT / "fixtures"
QMP_SOCK = Path("/tmp/md98_win98_qmp.sock")
SERIAL_SOCK = Path("/tmp/md98_win98_serial.sock")


class QmpClient:
    def __init__(self, sock_path: Path, timeout: float = 15.0):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(timeout)
        deadline = time.time() + timeout
        while True:
            try:
                self.sock.connect(str(sock_path))
                break
            except OSError:
                if time.time() > deadline:
                    raise
                time.sleep(0.1)
        self.f = self.sock.makefile("rwb", buffering=0)
        self._read_json()  # QMP greeting
        self.cmd("qmp_capabilities")

    def _read_json(self) -> dict:
        line = self.f.readline()
        if not line:
            raise EOFError("QMP connection closed")
        return json.loads(line.decode("utf-8"))

    def cmd(self, execute: str, arguments: dict | None = None) -> dict:
        payload = {"execute": execute}
        if arguments is not None:
            payload["arguments"] = arguments
        self.f.write((json.dumps(payload) + "\n").encode("utf-8"))
        while True:
            msg = self._read_json()
            if "return" in msg or "error" in msg:
                return msg

    def screendump(self, png_path: Path) -> Path:
        ppm_path = png_path.with_suffix(".ppm")
        res = self.cmd("screendump", {"filename": str(ppm_path)})
        if "error" in res:
            raise RuntimeError(f"QMP screendump error: {res}")
        img = Image.open(ppm_path)
        img.save(png_path, "PNG")
        try:
            ppm_path.unlink()
        except OSError:
            pass
        return png_path

    def send_key(self, keys: list[str], hold_time: int = 80):
        qkeys = [{"type": "qcode", "data": k} for k in keys]
        self.cmd("send-key", {"keys": qkeys, "hold-time": hold_time})

    def type_text(self, text: str, delay: float = 0.06):
        shift_map = {
            "!": "1", "@": "2", "#": "3", "$": "4", "%": "5",
            "^": "6", "&": "7", "*": "8", "(": "9", ")": "0",
            "_": "minus", "+": "equal", "{": "bracket_left", "}": "bracket_right",
            "|": "backslash", ":": "semicolon", '"': "apostrophe",
            "<": "comma", ">": "dot", "?": "slash", "~": "grave_accent",
        }
        plain_map = {
            " ": "spc", "\n": "ret", "\t": "tab", "-": "minus", "=": "equal",
            "[": "bracket_left", "]": "bracket_right", "\\": "backslash",
            ";": "semicolon", "'": "apostrophe", ",": "comma", ".": "dot",
            "/": "slash", "`": "grave_accent",
        }
        for ch in text:
            if "a" <= ch <= "z" or "0" <= ch <= "9":
                self.send_key([ch])
            elif "A" <= ch <= "Z":
                self.send_key(["shift", ch.lower()])
            elif ch in shift_map:
                self.send_key(["shift", shift_map[ch]])
            elif ch in plain_map:
                self.send_key([plain_map[ch]])
            time.sleep(delay)

    def close(self):
        try:
            self.f.close()
            self.sock.close()
        except OSError:
            pass


class SerialClient:
    def __init__(self, sock_path: Path, timeout: float = 15.0):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(timeout)
        deadline = time.time() + timeout
        while True:
            try:
                self.sock.connect(str(sock_path))
                break
            except OSError:
                if time.time() > deadline:
                    raise
                time.sleep(0.1)
        self.buf = b""

    def send_line(self, line: str):
        self.sock.sendall((line + "\r\n").encode("ascii"))

    def read_until(self, markers: tuple[str, ...], timeout: float = 30.0) -> str:
        deadline = time.time() + timeout
        out_lines: list[str] = []
        self.sock.settimeout(0.25)
        while time.time() < deadline:
            try:
                chunk = self.sock.recv(4096)
                if not chunk:
                    break
                self.buf += chunk
            except socket.timeout:
                pass
            while b"\n" in self.buf:
                raw_line, self.buf = self.buf.split(b"\n", 1)
                text = raw_line.decode("ascii", errors="replace").strip()
                if text:
                    out_lines.append(text)
                    if any(text.startswith(m) for m in markers):
                        return "\n".join(out_lines)
        return "\n".join(out_lines)

    def drain(self, duration: float = 0.3) -> str:
        deadline = time.time() + duration
        out = []
        self.sock.settimeout(0.1)
        while time.time() < deadline:
            try:
                chunk = self.sock.recv(4096)
                if not chunk:
                    break
                out.append(chunk.decode("ascii", errors="replace"))
            except socket.timeout:
                pass
        return "".join(out)

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass


def build_dist_iso(iso_path: Path, extra_files: list[Path] | None = None) -> Path:
    """Create a Joliet ISO image with dist/ binaries and fixtures/."""
    with tempfile.TemporaryDirectory(prefix="md98_iso_") as staging:
        stage = Path(staging)
        if DIST_DIR.exists():
            for p in DIST_DIR.iterdir():
                if p.is_file():
                    shutil.copy2(p, stage / p.name)
        fix_dst = stage / "fixtures"
        fix_dst.mkdir(exist_ok=True)
        if FIXTURES_DIR.exists():
            for p in FIXTURES_DIR.iterdir():
                if p.is_file():
                    shutil.copy2(p, fix_dst / p.name)
        if extra_files:
            for p in extra_files:
                if p.is_file():
                    shutil.copy2(p, fix_dst / p.name)

        subprocess.run(
            ["genisoimage", "-quiet", "-J", "-r", "-V", "MD98DIST", "-o", str(iso_path), str(stage)],
            check=True,
        )
    return iso_path


def win98_build_and_audit(package: str = "md98-win32", bin_name: str = "md98") -> dict:
    """Run scripts/build_win98.sh and return build + PE audit results."""
    proc = subprocess.run(
        [str(REPO_ROOT / "scripts" / "build_win98.sh"), package, bin_name],
        cwd=str(REPO_ROOT),
        capture_output=True,
        text=True,
    )
    audit_json = None
    exe_path = DIST_DIR / f"{bin_name}.exe"
    if exe_path.exists():
        aud = subprocess.run(
            [sys.executable, str(REPO_ROOT / "infra" / "audit_pe.py"), "--json", str(exe_path)],
            capture_output=True,
            text=True,
        )
        if aud.stdout.strip():
            try:
                audit_json = json.loads(aud.stdout)
            except json.JSONDecodeError:
                pass
    return {
        "returncode": proc.returncode,
        "passed": proc.returncode == 0,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
        "audit": audit_json,
    }


def win98_run_e2e(
    fixture_path: str | None = None,
    wait_ms: int = 3500,
    extra_args: str = "",
    screenshot_out: str | None = None,
    send_keys_after_ms: int | None = None,
    keys_to_type: str | None = None,
    key_combos: list[list[str]] | None = None,
    snapshot_name: str = "ready_desktop",
) -> dict:
    """Restore Win98 SE snapshot (-m 40 -cpu pentiumpro), inject ISO, run md98.exe, capture screenshot & logs."""
    if not QCOW2_PATH.exists():
        return {"passed": False, "error": f"QCOW2 image not found: {QCOW2_PATH}"}

    for sock in (QMP_SOCK, SERIAL_SOCK):
        try:
            sock.unlink()
        except OSError:
            pass

    iso_path = Path("/tmp/md98_dist.iso")
    extra_files = []
    guest_fixture = None
    if fixture_path:
        fp = Path(fixture_path)
        if not fp.is_absolute():
            fp = REPO_ROOT / fp
        if fp.exists():
            extra_files.append(fp)
            guest_fixture = f"D:\\fixtures\\{fp.name}"

    build_dist_iso(iso_path, extra_files=extra_files)

    # Create an ephemeral copy-on-write clone so win98se.qcow2 is never modified
    # (works identically to -snapshot without requiring writable /var/tmp).
    ephemeral_dir = REPO_ROOT / "infra" / "qemu" / "work"
    ephemeral_dir.mkdir(parents=True, exist_ok=True)
    ephemeral_qcow2 = ephemeral_dir / f"ephemeral_{os.getpid()}.qcow2"
    subprocess.run(["cp", "--reflink=auto", str(QCOW2_PATH), str(ephemeral_qcow2)], check=True)

    qemu_cmd = [
        "qemu-system-i386",
        "-M", "pc",
        "-cpu", "pentium2,-mmx,-sse,-sse2",
        "-m", "40",
        "-vga", "cirrus",
        "-drive", f"file={ephemeral_qcow2},format=qcow2,if=ide,index=0",
        "-drive", "id=cd0,if=ide,index=2,media=cdrom,readonly=on",
        "-serial", f"unix:{SERIAL_SOCK},server,nowait",
        "-qmp", f"unix:{QMP_SOCK},server,nowait",
        "-display", "none",
        "-loadvm", snapshot_name,
    ]

    env = os.environ.copy()
    env["TMPDIR"] = "/tmp"
    proc = subprocess.Popen(qemu_cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
    logs: list[str] = []
    shot_path = Path(screenshot_out) if screenshot_out else Path(f"/tmp/md98_e2e_{int(time.time())}.png")

    try:
        qmp = QmpClient(QMP_SOCK, timeout=20.0)
        ser = SerialClient(SERIAL_SOCK, timeout=20.0)

        # Insert freshly built dist ISO into empty CD-ROM drive (D:)
        chg_res = qmp.cmd("blockdev-change-medium", {
            "device": "cd0",
            "filename": str(iso_path),
            "format": "raw",
        })
        if "error" in chg_res:
            logs.append(f"[cdrom] {chg_res}")
        time.sleep(2.0)

        # Ping in-guest serial agent
        ser.drain(0.3)
        ser.send_line("PING")
        pong = ser.read_until(("PONG",), timeout=10.0)
        logs.append(f"[ping] {pong}")
        if "PONG" not in pong:
            # Retry once in case snapshot woke mid-read
            ser.send_line("PING")
            pong = ser.read_until(("PONG",), timeout=10.0)
            logs.append(f"[ping2] {pong}")

        # Build guest command (account for live typing duration so screenshot happens before exit)
        typing_extra_ms = int(len(keys_to_type) * 180) if keys_to_type else 0
        headless_ms = max(wait_ms + typing_extra_ms + 2000, 3500)
        cmd_parts = ["D:\\md98.exe"]
        if guest_fixture:
            cmd_parts.extend(["--test-fixture", guest_fixture])
        if extra_args:
            cmd_parts.append(extra_args)
        cmd_parts.extend(["--headless-exit", str(headless_ms)])
        guest_cmd = " ".join(cmd_parts)

        ser.send_line(f"RUN {guest_cmd}")
        started = ser.read_until(("STARTED", "ERR"), timeout=15.0)
        logs.append(f"[start] {started}")

        if send_keys_after_ms and (keys_to_type or key_combos):
            time.sleep(send_keys_after_ms / 1000.0)
            if keys_to_type:
                qmp.type_text(keys_to_type)
            if key_combos:
                for combo in key_combos:
                    qmp.send_key(combo)
                    time.sleep(0.15)
            # Wait for Win98 keyboard queue drain + 250ms debounce + repaint
            time.sleep(2.8)
        else:
            time.sleep(wait_ms / 1000.0)

        # Capture QMP screenshot while md98.exe is displaying on screen
        qmp.screendump(shot_path)

        # Wait for process exit & memory status
        exited = ser.read_until(("EXITED", "TIMEOUT", "ERR"), timeout=max(20.0, headless_ms / 1000.0 + 10.0))
        logs.append(f"[exit] {exited}")
        mem = ser.read_until(("MEM",), timeout=3.0)
        if mem:
            logs.append(f"[mem] {mem}")

        # Also read metrics log if --dump-metrics created C:\MD98LOG.TXT
        if "--dump-metrics" in extra_args:
            ser.send_line("READ C:\\MD98LOG.TXT")
            metrics_out = ser.read_until(("END_FILE", "ERR"), timeout=5.0)
            logs.append(f"[metrics]\n{metrics_out}")

        qmp.cmd("quit")
        qmp.close()
        ser.close()
        proc.wait(timeout=10)

        full_log = "\n".join(logs)
        exit_ok = "EXITED" in full_log and "code=0 " in full_log
        return {
            "passed": exit_ok,
            "guest_cmd": guest_cmd,
            "screenshot": str(shot_path),
            "serial_log": full_log,
        }
    except Exception as e:
        return {
            "passed": False,
            "error": str(e),
            "screenshot": str(shot_path) if shot_path.exists() else None,
            "serial_log": "\n".join(logs),
        }
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
        try:
            ephemeral_qcow2.unlink()
        except OSError:
            pass


def run_mcp_server():
    from mcp.server.fastmcp import FastMCP

    mcp = FastMCP("win98-mcp")

    @mcp.tool()
    def tool_win98_build_and_audit(package: str = "md98-win32", bin_name: str = "md98") -> str:
        """Cross-compile md98 for Windows 98 SE (Pentium Pro) and run the static PE32 import/ISA audit."""
        return json.dumps(win98_build_and_audit(package=package, bin_name=bin_name), indent=2)

    @mcp.tool()
    def tool_win98_run_e2e(fixture_path: str = "", wait_ms: int = 3500, extra_args: str = "") -> str:
        """Restore Win98 SE snapshot (-m 40 -cpu pentiumpro), run md98.exe, capture QMP PNG screenshot and serial logs."""
        res = win98_run_e2e(
            fixture_path=fixture_path or None,
            wait_ms=wait_ms,
            extra_args=extra_args,
        )
        return json.dumps(res, indent=2)

    @mcp.tool()
    def tool_win98_send_keys(keys: str, fixture_path: str = "", wait_ms: int = 4500) -> str:
        """Launch md98.exe on Win98 SE, type keystrokes via QMP, and return updated screenshot and logs."""
        res = win98_run_e2e(
            fixture_path=fixture_path or None,
            wait_ms=wait_ms,
            send_keys_after_ms=1500,
            keys_to_type=keys,
        )
        return json.dumps(res, indent=2)

    mcp.run()


def main():
    parser = argparse.ArgumentParser(description="md98 Win98 SE QEMU/QMP harness & MCP server")
    sub = parser.add_subparsers(dest="cmd")

    p_build = sub.add_parser("build-audit")
    p_build.add_argument("--package", default="md98-win32")
    p_build.add_argument("--bin-name", default="md98")

    p_e2e = sub.add_parser("run-e2e")
    p_e2e.add_argument("--fixture", default=None)
    p_e2e.add_argument("--wait-ms", type=int, default=3500)
    p_e2e.add_argument("--extra-args", default="")
    p_e2e.add_argument("--screenshot", default=None)
    p_e2e.add_argument("--type", dest="keys_to_type", default=None)
    p_e2e.add_argument("--send-after-ms", type=int, default=1500)

    sub.add_parser("mcp")
    args = parser.parse_args()

    if args.cmd == "build-audit":
        res = win98_build_and_audit(args.package, args.bin_name)
        print(json.dumps(res, indent=2))
        sys.exit(0 if res["passed"] else 1)
    elif args.cmd == "run-e2e":
        res = win98_run_e2e(
            fixture_path=args.fixture,
            wait_ms=args.wait_ms,
            extra_args=args.extra_args,
            screenshot_out=args.screenshot,
            send_keys_after_ms=args.send_after_ms if args.keys_to_type else None,
            keys_to_type=args.keys_to_type,
        )
        print(json.dumps(res, indent=2))
        sys.exit(0 if res["passed"] else 1)
    else:
        run_mcp_server()


if __name__ == "__main__":
    main()
