# AGENTS.md — md98 (Windows 98 SE GFM Editor & Previewer)

This repository contains **md98**, a native GitHub Flavored Markdown (GFM) editor and live previewer written in Rust for **Windows 98 SE**.

All autonomous agents (including Google Antigravity) must strictly adhere to the hardware limits, ABI rules, and verification gates defined below.

## 1. Target Hardware & OS Constraints (Non-Negotiable)
- **OS:** Windows 98 Second Edition (4.10.2222 A). PE32 Subsystem & OS version must be `4.0`.
- **CPU:** Intel Pentium Pro (150–200 MHz, `i686`). **NO MMX, NO SSE, NO SSE2.**
- **RAM:** 40 MB total (~15–20 MB free after OS boot). Target < 4 MB working set.
- **Win32 API:** Native ANSI (`*A`) only. No `unicows.dll`. Convert UTF-8 ↔ CP1252 at the Win32 boundary.
- **Graphics:** 16-bit GDI heap, 800×600 @ 16-bit color. Cache all GDI objects; zero per-frame leaks.

## 2. Toolchain & Allowed Imports
- Build `md98-win32` exclusively via `scripts/build_win98.sh` (Dockerized `rust9x` + `rust-lld` + `r9xrsrt`).
- Enforce `-C target-cpu=pentiumpro -C target-feature=-mmx,-sse,-sse2` in `.cargo/config.toml`.
- Keep `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `strip = true`.
- **Allowed DLLs:** `KERNEL32.DLL`, `USER32.DLL`, `GDI32.DLL`, `ADVAPI32.DLL`, `COMDLG32.DLL`, `COMCTL32.DLL`, `RICHED20.DLL`.

## 3. Architectural Invariants
- Keep parsing, wrapping, and `DrawOp` generation in `md98-core` behind `TextMeasurer`.
- Use `RichEdit 2.0` (`RICHED20.DLL`), never the Win32 `EDIT` control (64 KB limit).
- Debounce `EN_CHANGE` with a 250–300 ms `WM_TIMER`.
- Size the double-buffer `CreateCompatibleBitmap` strictly to the visible viewport.
- Reuse `Vec<LayoutBlock>` and `Vec<DrawOp>` via `.clear()` across edits.

## 4. Definition of Done
A task is complete only when all four gates pass:
1. `cargo test -p md98-core` passes with zero warnings.
2. `win98_build_and_audit()` succeeds with zero disallowed DLLs or missing Win98 SE exports.
3. `win98_run_e2e()` executes cleanly on the `-m 40 -cpu pentiumpro` QEMU VM with zero crashes, zero GDI handle leaks, and visually verified QMP screenshots.
4. Antigravity Implementation Plan and Walkthrough Artifacts (with screenshots) are generated.
