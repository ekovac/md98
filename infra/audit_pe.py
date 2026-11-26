#!/usr/bin/env python3
"""Static PE32 import, header, and ISA auditor for Windows 98 SE (Pentium Pro)."""

import argparse
import json
import os
import re
import subprocess
import sys

try:
    import pefile
except ImportError:
    venv_py = os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
        ".venv",
        "bin",
        "python3",
    )
    if os.path.exists(venv_py) and os.path.abspath(sys.executable) != os.path.abspath(venv_py):
        os.execv(venv_py, [venv_py] + sys.argv)
    print("ERROR: Python 'pefile' module not found.", file=sys.stderr)
    sys.exit(2)


# Forbidden MMX / SSE / SSE2 / SSE3 / AVX mnemonics for Intel Pentium Pro (i686).
FORBIDDEN_MNEMONIC_PATTERNS = [
    r"\b(?:emms|femms)\b",
    r"\b(?:movd|movq|packsswb|packssdw|packuswb|punpck\w+|pmaddwd|pmulhw|pmullw)\b",
    r"\b(?:padd\w+|psub\w+|pand|pandn|por|pxor|psll\w+|psrl\w+|psra\w+|pcmpeq\w+|pcmpgt\w+)\b",
    r"\b(?:movaps|movups|movss|movhps|movlps|movhlps|movlhps|movmskps)\b",
    r"\b(?:movapd|movupd|movsd|movhpd|movlpd|movmskpd|movdqa|movdqu|movdq2q|movq2dq)\b",
    r"\b(?:addps|addss|subps|subss|mulps|mulss|divps|divss|sqrtps|sqrtss|rsqrtps|rsqrtss|rcpps|rcpss)\b",
    r"\b(?:addpd|addsd|subpd|subsd|mulpd|mulsd|divpd|divsd|sqrtpd|sqrtsd)\b",
    r"\b(?:maxps|maxss|minps|minss|maxpd|maxsd|minpd|minsd|andps|andnps|orps|xorps|andpd|andnpd|orpd|xorpd)\b",
    r"\b(?:cmpps|cmpss|comiss|ucomiss|cmppd|cmpsd|comisd|ucomisd)\b",
    r"\b(?:cvtpi2ps|cvtsi2ss|cvtps2pi|cvttps2pi|cvtss2si|cvttss2si)\b",
    r"\b(?:cvtpd2pi|cvttpd2pi|cvtpi2pd|cvtpd2dq|cvttpd2dq|cvtdq2pd|cvtps2pd|cvtpd2ps|cvtss2sd|cvtsd2ss|cvtsd2si|cvttsd2si|cvtsi2sd|cvtdq2ps|cvtps2dq|cvttps2dq)\b",
    r"\b(?:shufps|unpckhps|unpcklps|shufpd|unpckhpd|unpcklpd|pshufw|pshufd|pshufhw|pshuflw)\b",
    r"\b(?:ldmxcsr|stmxcsr|sfence|lfence|mfence|clflush|maskmovq|maskmovdqu|movntq|movntps|movntpd|movntdq|movnti)\b",
    r"\b(?:prefetchnta|prefetcht0|prefetcht1|prefetcht2)\b",
    r"\b(?:%xmm[0-7]|%mm[0-7]|xmm[0-7]|mm[0-7])\b",
]
FORBIDDEN_RE = re.compile("|".join(FORBIDDEN_MNEMONIC_PATTERNS), re.IGNORECASE)


def audit_isa(pe_path: str) -> list[str]:
    """Scan disassembled .text section for MMX/SSE/SSE2 instructions."""
    errors = []
    objdump = None
    for candidate in ("objdump", "llvm-objdump"):
        if subprocess.run(["which", candidate], capture_output=True).returncode == 0:
            objdump = candidate
            break
    if not objdump:
        return errors

    cmd = [objdump, "-d", "--no-show-raw-insn", pe_path]
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode != 0:
        return errors

    bad_hits = []
    for line in res.stdout.splitlines():
        if "\t" not in line:
            continue
        insn = line.split("\t", 1)[1].strip()
        if not insn or insn.startswith("."):
            continue
        # Ignore string/data pseudo-ops and standard rep movsd (string move, NOT SSE2 movsd with xmm)
        if re.match(r"^(?:repz?\s+)?movs[bwdl]?(?:\s|$)", insn) and "xmm" not in insn.lower():
            continue
        if FORBIDDEN_RE.search(insn):
            bad_hits.append(line.strip())
            if len(bad_hits) >= 20:
                break

    for hit in bad_hits:
        errors.append(f"Forbidden MMX/SSE instruction detected: {hit}")
    return errors


def audit_pe(pe_path: str, allowlist_path: str, max_size_kb: int | None = None, check_isa: bool = True) -> dict:
    with open(allowlist_path, "r") as f:
        allowlist = json.load(f)

    allowed_dlls = {d.upper(): allowlist["dlls"][d.upper()] for d in allowlist["allowed_dlls"]}
    errors: list[str] = []
    warnings: list[str] = []

    if not os.path.exists(pe_path):
        raise FileNotFoundError(pe_path)

    file_size = os.path.getsize(pe_path)
    pe = pefile.PE(pe_path)

    # 1. Check PE32 (32-bit x86)
    magic = pe.OPTIONAL_HEADER.Magic
    machine = pe.FILE_HEADER.Machine
    if magic != 0x10B:
        errors.append(f"Not a PE32 binary: OptionalHeader.Magic = 0x{magic:x} (expected 0x10b)")
    if machine != 0x14C:
        errors.append(f"Not i386 machine type: Machine = 0x{machine:x} (expected 0x14c)")

    # 2. Check Subsystem & OS Version == 4.0
    subsys_maj = pe.OPTIONAL_HEADER.MajorSubsystemVersion
    subsys_min = pe.OPTIONAL_HEADER.MinorSubsystemVersion
    os_maj = pe.OPTIONAL_HEADER.MajorOperatingSystemVersion
    os_min = pe.OPTIONAL_HEADER.MinorOperatingSystemVersion
    subsystem = pe.OPTIONAL_HEADER.Subsystem

    if (subsys_maj, subsys_min) != (4, 0):
        errors.append(f"SubsystemVersion is {subsys_maj}.{subsys_min} (expected 4.0)")
    if (os_maj, os_min) != (4, 0):
        errors.append(f"OperatingSystemVersion is {os_maj}.{os_min} (expected 4.0)")

    # 3. Check imports against Win98 SE allowlist
    imported_summary = {}
    if hasattr(pe, "DIRECTORY_ENTRY_IMPORT"):
        for entry in pe.DIRECTORY_ENTRY_IMPORT:
            dll_name = entry.dll.decode("ascii", errors="replace").upper()
            syms = []
            if dll_name not in allowed_dlls:
                errors.append(f"Disallowed DLL import: {dll_name}")
            dll_exports = allowed_dlls.get(dll_name)
            allowed_names = set(dll_exports["names"]) if dll_exports else set()
            allowed_ordinals = set(dll_exports["ordinals"]) if dll_exports else set()

            for imp in entry.imports:
                if imp.import_by_ordinal:
                    sym_label = f"#{imp.ordinal}"
                    syms.append(sym_label)
                    if dll_exports is not None and imp.ordinal not in allowed_ordinals:
                        errors.append(f"Missing Win98 SE export: {dll_name}:{sym_label}")
                else:
                    sym_name = imp.name.decode("ascii", errors="replace")
                    syms.append(sym_name)
                    if dll_exports is not None and sym_name not in allowed_names:
                        errors.append(f"Missing Win98 SE export: {dll_name}!{sym_name}")
            imported_summary[dll_name] = sorted(syms)

    # Check delay-load imports too (must not hide disallowed DLLs!)
    if hasattr(pe, "DIRECTORY_ENTRY_DELAY_IMPORT"):
        for entry in pe.DIRECTORY_ENTRY_DELAY_IMPORT:
            dll_name = entry.dll.decode("ascii", errors="replace").upper()
            errors.append(f"Disallowed delay-load DLL import: {dll_name}")

    # 4. Check binary size budget
    if max_size_kb is not None and file_size > max_size_kb * 1024:
        warnings.append(f"Binary size {file_size} bytes exceeds soft budget {max_size_kb} KB")

    # 5. Check for MMX/SSE/SSE2 instructions
    if check_isa:
        errors.extend(audit_isa(pe_path))

    return {
        "path": pe_path,
        "size_bytes": file_size,
        "size_kb": round(file_size / 1024.0, 2),
        "machine": hex(machine),
        "subsystem": subsystem,
        "subsystem_version": f"{subsys_maj}.{subsys_min}",
        "os_version": f"{os_maj}.{os_min}",
        "imports": imported_summary,
        "warnings": warnings,
        "errors": errors,
        "passed": len(errors) == 0,
    }


def main():
    parser = argparse.ArgumentParser(description="Audit PE32 binary for Windows 98 SE Pentium Pro compatibility")
    parser.add_argument("pe_path", help="Path to PE32 .exe to audit")
    parser.add_argument(
        "--allowlist",
        default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "win98se_exports.json"),
        help="Path to win98se_exports.json",
    )
    parser.add_argument("--max-size-kb", type=int, default=512, help="Soft binary size budget in KB")
    parser.add_argument("--no-isa-check", action="store_true", help="Skip objdump MMX/SSE instruction scan")
    parser.add_argument("--json", action="store_true", help="Output JSON report")
    args = parser.parse_args()

    report = audit_pe(args.pe_path, args.allowlist, max_size_kb=args.max_size_kb, check_isa=not args.no_isa_check)

    if args.json:
        print(json.dumps(report, indent=2))
    else:
        print(f"PE Audit: {report['path']}")
        print(f"  Size:       {report['size_bytes']} bytes ({report['size_kb']} KB)")
        print(f"  Subsystem:  {report['subsystem']} (version {report['subsystem_version']})")
        print(f"  OS Version: {report['os_version']}")
        print(f"  DLLs ({len(report['imports'])}):")
        for dll, syms in sorted(report["imports"].items()):
            print(f"    - {dll}: {len(syms)} symbols")
        for w in report["warnings"]:
            print(f"  WARN:  {w}")
        for e in report["errors"]:
            print(f"  ERROR: {e}")
        print(f"  RESULT: {'PASS' if report['passed'] else 'FAIL'}")

    sys.exit(0 if report["passed"] else 1)


if __name__ == "__main__":
    main()
