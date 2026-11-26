# Win98 GFM Editor & Previewer (`md98`): Design, Implementation & Agentic Handoff Plan

This document specifies the architecture, toolchain, hardware constraints, phased implementation roadmap, and autonomous verification infrastructure for building **md98**, a native GitHub Flavored Markdown (GFM) editor and live previewer for **Windows 98 SE** written in Rust. It is structured for direct handoff to **Google Antigravity**.

---

## 1. Target Hardware & OS Constraints (Non-Negotiable)

The target machine is a **Pentium Pro with 40 MB of RAM** running **Windows 98 Second Edition (4.10.2222 A)**. Every architectural and toolchain decision is bound by these limits:

| Parameter | Target Reality | Architectural Impact |
| :--- | :--- | :--- |
| **CPU / ISA** | Intel Pentium Pro (150–200 MHz, `i686`) | **No MMX, no SSE, no SSE2.** Any emitted MMX/SSE instruction triggers an immediate Illegal Instruction fault (`0xC000001D`). Lock Rust to `-C target-cpu=pentiumpro` and `-C target-feature=-mmx,-sse,-sse2`. |
| **RAM** | 40 MB total (~15–20 MB free after Win98 SE boot) | Target < 4 MB working set. Avoid browser engines, DOM trees, or full-document bitmaps. Reuse flat layout arenas and keep the `.exe` under ~500 KB. |
| **PE / ABI** | 32-bit PE32, Subsystem & OS Version `4.0` | Link with `rust-lld` using `--subsystem=windows:4.0`. |
| **Win32 API** | Native ANSI (`*A`); `*W` stubbed without `unicows.dll` | Store text internally as UTF-8; convert to/from active codepage (CP1252) via `MultiByteToWideChar` / `WideCharToMultiByte` at the Win32 boundary. |
| **Graphics** | GDI only (no Direct2D, DirectWrite, or WebView2); 16-bit GDI heap; 800×600 @ 16-bit color | Custom double-buffered GDI layout & paint engine. Strictly cache all `HFONT`, `HPEN`, and `HBRUSH` objects; zero per-frame GDI allocations. |
| **Rust `std`** | Upstream Rust dropped Win9x/XP/7; `std` calls NT6+ APIs | Use the **Rust9x** (`rust9x/rust`) Tier-4 toolchain fork. |

---

## 2. Toolchain & Compilation Strategy

### Primary Toolchain: Rust9x
* **Compiler:** Pinned Dockerized `rust9x/rust` (providing `std` fallbacks for Windows 95/98/ME) invoked via `scripts/build_win98.sh`.
* **Linker & Runtime:** `rust-lld` (`--subsystem=windows:4.0`) paired with `r9xrsrt` (minimal static MSVC runtime replacement) so `md98.exe` is a single standalone binary with zero external `MSVCRT` DLL dependencies.
* **CPU Flags (`.cargo/config.toml`):**
  ```toml
  [target.i686-win9x-windows-msvc]
  rustflags = [
    "-C", "target-cpu=pentiumpro",
    "-C", "target-feature=-mmx,-sse,-sse2",
  ]
  ```
* **Size Optimization (`Cargo.toml`):** Set `opt-level = "s"`, `lto = true`, `codegen-units = 1`, and `strip = true` to keep the binary footprint under ~500 KB.
* **Allowed System DLLs:** Strictly limit imports to stock Windows 98 SE libraries:
  * `KERNEL32.DLL`
  * `USER32.DLL`
  * `GDI32.DLL`
  * `COMDLG32.DLL`
  * `COMCTL32.DLL`
  * `RICHED20.DLL`

### Static PE Import Gate (Critical)
A binary can compile cleanly with `rust9x` yet crash on Windows 98 with *"The MD98.EXE file is linked to missing export KERNEL32.DLL:..."* if a transitive crate calls a Win2000/XP+ API.
* Maintain a curated `infra/win98se_exports.json` allowlist extracted from clean Windows 98 SE system DLLs.
* Run `infra/audit_pe.py` (using Python `pefile`) after every build. The build **must fail** if any imported DLL or symbol is absent from the allowlist or if the PE subsystem version is not `4.0`.

---

## 3. Application Architecture

Separate the application into a two-crate Cargo workspace so core parsing and layout logic can be tested natively on the host in milliseconds.

### Crate 1: `md98-core` (Pure Rust, Host-Testable)
* **GFM Parser:** `pulldown-cmark` with `Options::ENABLE_TABLES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS | ENABLE_SMART_PUNCTUATION`.
* **Layout IR:** Transforms the `pulldown-cmark` event stream into flat `Vec<LayoutBlock>` and `Vec<DrawOp>` buffers (positioned text runs, rectangles, borders, checkboxes, horizontal rules).
* **Buffer Reuse:** Keep `Vec<LayoutBlock>` and `Vec<DrawOp>` alive across edits and call `.clear()` instead of reallocating to prevent heap fragmentation within 40 MB RAM.
* **Text Measurement Trait:** Define `trait TextMeasurer { fn measure(&self, text: &str, style: FontStyle) -> (i32, i32); }`. On Windows 98, this wraps `GetTextExtentExPointA`. In host unit tests, a deterministic mock measurer enables fast, repeatable word-wrap and table-sizing tests.

### Crate 2: `md98-win32` (Win98 Binary)
* **Main Window (Native Win32 SDI):** Menu Bar (`File`, `Edit`, `View`, `Help`), Toolbar (`COMCTL32`), and Status Bar (line/column, byte size, parse/layout time in ms).
* **Left Pane (Editor):** `RichEdit 2.0` (`RICHED20.DLL`, included in Win98 SE). *Never use the standard Win32 `EDIT` control*, which has a hard 64 KB limit on Win9x.
* **Right Pane (Preview):** Custom Win32 child window class (`MD98_PREVIEW`) with native vertical/horizontal scrollbars (`WS_VSCROLL | WS_HSCROLL`).
* **Splitter:** Custom draggable 4px vertical divider with `IDC_SIZEWE` cursor and live or XOR-bar resize.
* **CPU-Friendly Debouncing:** On `EN_CHANGE` from `RichEdit`, reset a **250–300 ms** Win32 `WM_TIMER` so rapid typing in large documents does not saturate the 150–200 MHz Pentium Pro.
* **Font & GDI Cache:** Pre-create and cache `HFONT` handles (Body `MS Sans Serif` / `Arial`, Monospace `Courier New`, Headings H1–H6, Bold/Italic/Strikeout variants), `HPEN`s, and `HBRUSH`es at startup.
* **Viewport-Only Double Buffering:** On `WM_PAINT`, size the offscreen `CreateCompatibleBitmap` strictly to the **visible preview pane** (~400 KB–1 MB), never the full scrolled height of the document. Render only `DrawOp`s intersecting the viewport, then `BitBlt` to the screen.
* **GFM Visual Elements:**
  * **Tables:** Two-pass column width calculation (min/max content width capped to viewport), rendered with 1px GDI borders and shaded header cells.
  * **Task Lists:** Drawn using Win32 `DrawFrameControl(..., DFC_BUTTON, DFCS_BUTTONCHECK | ...)` for authentic Win98 checkboxes.
  * **Code Blocks:** Shaded `#F0F0F0` background rectangle, 1px border, `Courier New` monospace font.
  * **Blockquotes:** Left 3px vertical accent bar with indented text.

### Forbidden Crates & Patterns
* **No modern GUI/async stacks:** `winit`, `eframe`, `egui`, `iced`, `tokio`, `async-std`, `reqwest`.
* **No NT-only Win32 APIs:** `SetWindowLongPtr`, `GetModuleHandleEx`, `InitializeSRWLock`, `Direct2D`, `DirectWrite`, `GDI+`.
* **No `unicows.dll` dependency:** Use `*A` Win32 APIs exclusively.
* **No per-frame GDI allocations:** Never call `CreateFontA` / `CreateSolidBrush` / `CreatePen` inside `WM_PAINT` without caching and matching `DeleteObject`.

---

## 4. Infrastructure for Fully Autonomous Agentic Development

Because Windows 98 cannot be emulated by native `cargo test`, autonomous development requires a **closed-loop verification harness** that Antigravity can execute without human intervention.

### Tier 1: Fast Host Loop (Seconds)
1. `cargo test -p md98-core` runs natively on the host to validate GFM parsing, table geometry, word wrapping, and UTF-8 ↔ CP1252 conversion.
2. `scripts/build_win98.sh` cross-compiles `md98-win32` inside the pinned Rust9x Docker container and immediately executes `infra/audit_pe.py`.

### Tier 2: Headless Windows 98 VM Loop (5–15 Seconds)
Use **QEMU** with a pre-installed, snapshotted **Windows 98 SE QCOW2 image** configured to match the real hardware:
* **Hardware-Accurate Flags:** `-m 40 -cpu pentiumpro -loadvm ready_desktop -snapshot`
  * `-m 40` enforces the 40 MB RAM ceiling.
  * `-cpu pentiumpro` traps any accidental MMX/SSE instructions emitted by Rust or a dependency.
  * `-loadvm ready_desktop -snapshot` restores a running desktop in ~2 seconds using an ephemeral overlay so the base image is never corrupted.
* **Binary & Fixture Injection:** Use QEMU's built-in user-mode SMB share (`-net user,smb=/path/to/dist`) or a virtual FAT32 floppy/secondary disk image populated via `mtools` (`mcopy`) before launch.
* **In-Guest Test Runner (`win98_agent.exe`):** A tiny C or Rust9x daemon running inside the snapshot listening on `COM1` (mapped to a host Unix socket via `-serial unix:/tmp/win98_serial.sock,server,nowait`).
  * Host sends: `RUN Z:\md98.exe --test-fixture Z:\fixtures\gfm_kitchen_sink.md --headless-exit 3000`
  * Guest launches `md98.exe`, captures exit code / crash status / structured debug logs, and streams them back over `COM1`.
* **Visual Verification (QMP Screendump):** Connect to QEMU's QMP socket (`-qmp unix:/tmp/win98_qmp.sock,server,nowait`) and issue `{"execute": "screendump", "arguments": {"filename": "/tmp/out.ppm"}}`, then convert to PNG. Antigravity's multimodal model inspects the screenshot directly to verify GFM rendering.
* **Input Simulation:** Issue QMP `send-key` / `input-send-event` commands to type Markdown live, drag the splitter, or scroll the preview pane.

### Tier 3: Antigravity Workspace Configuration
* **Custom MCP Server (`infra/win98-mcp/`):** Exposes structured tools to Antigravity so the agent never writes brittle shell one-liners:
  * `win98_build_and_audit()` → returns compiler output + PE import table check.
  * `win98_run_e2e(fixture_path, wait_ms)` → restores snapshot, injects binary, runs `md98.exe`, returns serial log + path to PNG screenshot.
  * `win98_send_keys(keys)` → sends keystrokes to the active VM and returns an updated screenshot.
* **Antigravity Skills (`.agent/skills/`):**
  * `win98-debug-crash/SKILL.md`: Procedure for diagnosing missing exports, illegal instructions, GDI leaks, or Win98 General Protection Faults using `audit_pe.py` and serial logs.
  * `gfm-visual-verify/SKILL.md`: Checklist for visually inspecting QMP screenshots against `fixtures/gfm_kitchen_sink.md`.
* **Artifact Policy:** Antigravity generates an **Implementation Plan Artifact** before each phase and a **Walkthrough Artifact** containing `audit_pe.py` output, serial metrics, and embedded QMP Win98 screenshots upon completion.

---

## 5. Phased Implementation Roadmap

Each phase is scoped as an independent Antigravity task with hard exit gates.

### Phase 0: Infrastructure & "Hello Win98"
* **Deliverables:** Dockerized `rust9x` build script, `win98se_exports.json` + `audit_pe.py`, QEMU snapshot harness (`-m 40 -cpu pentiumpro`), `win98-mcp` server, and a minimal Win32 `MessageBoxA` binary.
* **Exit Gate:** Antigravity compiles the binary, passes the PE audit, boots the QEMU snapshot, and produces a Walkthrough Artifact with a Win98 screenshot showing the message box.

### Phase 1: Native Win32 Shell & Editor
* **Deliverables:** Main SDI window, Menu Bar, Status Bar, draggable splitter, `RichEdit 2.0` left pane, empty `MD98_PREVIEW` right pane, `File -> Open / Save / Save As` (`GetOpenFileNameA` / `GetSaveFileNameA`), and UTF-8 ↔ CP1252 conversion.
* **Exit Gate:** Agent opens a >100 KB `.md` file in the VM without hitting the 64 KB `EDIT` limit and verifies typing + saving.

### Phase 2: `md98-core` Parser & Layout IR
* **Deliverables:** `pulldown-cmark` integration, `TextMeasurer` trait, word-wrapping engine, and `LayoutBlock` / `DrawOp` generation for headings, paragraphs, inline styles, lists, blockquotes, and code blocks.
* **Exit Gate:** 100% passing host unit tests covering edge cases (long unbroken URLs, deeply nested lists, empty documents).

### Phase 3: Double-Buffered GDI Preview Renderer
* **Deliverables:** `HFONT` cache, `GetTextExtentExPointA` measurer, viewport-only offscreen double-buffered `WM_PAINT`, vertical scrolling (`WM_VSCROLL`, `WM_MOUSEWHEEL`), and 250–300 ms debounced live updates from `RichEdit`.
* **Exit Gate:** QMP screenshot of `fixtures/basic_markdown.md` rendered cleanly with no flicker or clipping; GDI handle count remains flat across 50 consecutive edits.

### Phase 4: Full GFM Support
* **Deliverables:** GFM tables (two-pass column sizing + grid borders), task lists (`DrawFrameControl` checkboxes), strikethrough (`lfStrikeOut`), autolinks (styled blue + underlined), horizontal rules, and proportional editor-to-preview scroll sync.
* **Exit Gate:** QMP screenshot of `fixtures/gfm_kitchen_sink.md` verified visually by Antigravity.

### Phase 5: Hardening & Retro Constraints Testing
* **Deliverables:** Stress testing in 800×600 resolution, 16-bit color mode, and the exact 40 MB RAM / Pentium Pro VM configuration. Add CLI flags (`--test-fixture`, `--dump-metrics`) for automated regression testing.
* **Exit Gate:** Renders a 50 KB GFM document cleanly within the 40 MB RAM budget with zero GDI/memory leaks and zero Illegal Instruction faults.

---

## 6. Repository Layout

```text
md98/
├── AGENTS.md                   # Core rules, Win98 constraints, Definition of Done
├── Cargo.toml                  # Workspace: md98-core, md98-win32
├── .cargo/
│   └── config.toml             # Pentium Pro (-mmx,-sse,-sse2) & Rust9x target flags
├── .agent/
│   └── skills/
│       ├── win98-debug-crash/SKILL.md
│       └── gfm-visual-verify/SKILL.md
├── crates/
│   ├── md98-core/              # Pure Rust GFM parser + layout IR + host tests
│   └── md98-win32/             # Win32 UI, RichEdit 2.0, GDI renderer
├── fixtures/
│   ├── basic_markdown.md
│   └── gfm_kitchen_sink.md
└── infra/
    ├── Dockerfile.rust9x       # Pinned Rust9x cross-compilation environment
    ├── audit_pe.py             # PE32 import table & subsystem validator
    ├── win98se_exports.json    # Clean Win98 SE DLL export allowlist
    ├── win98_agent/            # Tiny in-guest serial test runner
    └── win98-mcp/              # MCP server wrapping QEMU QMP + serial runner
```

