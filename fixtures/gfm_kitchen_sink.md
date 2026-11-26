# md98 — GFM Kitchen Sink

Testing **GitHub Flavored Markdown** on *Windows 98 SE* (Pentium Pro, 40 MB RAM).

## Task List

- [x] Boot Windows 98 SE under `-m 40 -cpu pentiumpro`
- [x] Cross-compile pure Rust9x binary with zero MMX/SSE
- [ ] Write retro documentation in **md98**
- [ ] Verify ~~broken~~ strikethrough and `inline_code()`

## Comparison Table

| Component | Target Limit | md98 Strategy |
| :--- | :---: | ---: |
| CPU | Pentium Pro (No SSE) | `-C target-cpu=pentiumpro` |
| RAM | 40 MB Total | Flat Arena < 4 MB |
| Editor | > 64 KB Files | `RICHED20.DLL` |
| Preview | 16-bit GDI Heap | Viewport Double-Buffer |

## Blockquote, Links & Code

> Check out https://github.com/rust9x/rust and [md98 repository](https://example.com/md98) for more details — including smart "quotes" and ellipsis...

```c
/* Authentic Win98 GDI loop */
BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
```

### Nested Lists & Rules

1. Compile `md98-core` natively on host
   - Run unit tests for tables and word wrap
   - Verify CP1252 smart punctuation mapping
2. Audit PE32 imports against `win98se_exports.json`
3. Capture QMP screenshot in QEMU

---

*All systems nominal on Windows 98 SE.*
