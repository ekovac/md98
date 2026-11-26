---
name: win98-debug-crash
description: Diagnose Windows 98 SE runtime crashes, missing DLL exports, Illegal Instruction faults (0xC000001D), GDI handle leaks, or GPFs in md98.exe using audit_pe.py, objdump, and win98_agent serial logs.
---

# Debugging Windows 98 SE Crashes & Compatibility Faults

Follow this checklist whenever `md98.exe` fails to launch, exits abnormally, crashes with a General Protection Fault / Illegal Instruction, or leaks GDI objects inside the Windows 98 SE QEMU VM (`-m 40 -cpu pentiumpro`).

## 1. Run Static PE & ISA Audit First
```bash
python3 infra/audit_pe.py dist/md98.exe
```
Check for:
- **Subsystem / OS Version**: Must be `4.0` / `4.0`. If `6.0` or `5.0`, verify `/SUBSYSTEM:WINDOWS,4.0` and `/OSVERSION:4.0` in `.cargo/config.toml`.
- **Disallowed DLLs**: Only `KERNEL32.DLL`, `USER32.DLL`, `GDI32.DLL`, `COMDLG32.DLL`, `COMCTL32.DLL`, `RICHED20.DLL` are permitted. If `MSVCRT.DLL`, `VCRUNTIME140.DLL`, `ADVAPI32.DLL`, `WS2_32.DLL`, or `UNICOWS.DLL` appear, check `-Zunicows=no`, `+crt-static`, `r9xrsrt` lib path, and transitive crates.
- **Missing Win98 SE Exports**: Compare offending symbol against `infra/win98se_exports.json`. Replace NT/2000/XP+ calls (e.g. `SetWindowLongPtrA`, `GetModuleHandleExA`, `InitializeCriticalSectionAndSpinCount`) with Win98 SE equivalents (`SetWindowLongA`, `GetModuleHandleA`, `InitializeCriticalSection`).
- **MMX/SSE/SSE2 Instructions (`0xC000001D`)**: `audit_pe.py` disassembles `.text` and flags any MMX/SSE opcode. Ensure both `r9xrsrt` and `md98-win32` are built with `-C target-cpu=pentiumpro -C target-feature=-mmx,-sse,-sse2` and `-Z build-std=std,panic_abort`.

## 2. Inspect Serial Logs from `win98_agent.exe`
- Look for `STARTED pid=...`, `EXITED pid=... code=...`, and `MEM load=... avail_phys=...`.
- Exit code `0xC000001D` (`3221225501`): Illegal Instruction (MMX/SSE emitted).
- Exit code `0xC0000005` (`3221225477`): Access Violation (check struct field alignment/packing for 32-bit x86 `#[repr(C)]` Win32 structs, null-terminated `*A` strings, or dangling `HFONT`/`HBITMAP` handles).
- Exit code `0xC00000FD`: Stack overflow (remember `/STACK:262144` — keep large buffers on the heap, never on the stack).

## 3. Diagnose GDI Leaks & 16-bit GDI Heap Exhaustion
- Check `--dump-metrics` output for `gdi_allocs`, `gdi_frees`, and live GDI handle delta across edits.
- Verify every `CreateCompatibleDC` has a matching `DeleteDC`, every `CreateCompatibleBitmap` has a matching `DeleteObject` (after restoring the original bitmap via `SelectObject`), and `HFONT`/`HPEN`/`HBRUSH` objects are cached once at startup and never created inside `WM_PAINT`.

## 4. Visual Failure Inspection
- Inspect the captured QMP PNG screenshot: Windows 98 displays modal dialog boxes for *"The MD98.EXE file is linked to missing export..."* or *"This program has performed an illegal operation and will be shut down"*.
