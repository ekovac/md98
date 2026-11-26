---
name: gfm-visual-verify
description: Visually inspect Windows 98 SE QMP screenshots of md98 rendering fixtures/basic_markdown.md and fixtures/gfm_kitchen_sink.md to verify GFM layout, typography, tables, task lists, code blocks, and retro UI cleanliness.
---

# GFM Visual Verification Checklist (Windows 98 SE)

When inspecting a QMP screenshot captured from the `800x600` (16-bit color) Windows 98 SE VM:

1. **No OS Error Dialogs**: Confirm no missing-export dialog, Illegal Operation dialog, or clipped window chrome.
2. **Window Chrome & Layout**:
   - Native Win98 title bar, menu bar (`File`, `Edit`, `View`, `Help`), status bar (line/col, byte size, layout time in ms).
   - Visible 4px vertical splitter dividing the left `RichEdit 2.0` editor pane and right `MD98_PREVIEW` pane.
   - Clean scrollbars with no overlapping text or phantom artifacts.
3. **Typography & Inline Styles**:
   - Headings `H1`–`H6` show clear size/weight hierarchy and bottom rule on `H1`/`H2`.
   - **Bold**, *italic*, ~~strikethrough~~, `inline code` (monospace shaded), and underlined blue links render legibly without overlapping glyphs.
   - CP1252 smart punctuation (`“ ”`, `‘ ’`, `—`, `…`, `•`) renders cleanly without mojibake.
4. **Block Elements**:
   - Bulleted and numbered lists (including nested lists) indent cleanly.
   - Blockquotes display a left 3px vertical accent bar with indented body text.
   - Fenced code blocks display a `#F0F0F0` shaded background, 1px border, and `Courier New` monospace text without clipping.
   - Horizontal rules (`---`) span the content width cleanly.
5. **GFM Extensions (`gfm_kitchen_sink.md`)**:
   - **Tables**: Header row shaded, 1px grid borders intact, columns sized cleanly within the preview pane without horizontal overflow or crushed text.
   - **Task Lists**: Authentic Win98 3D checkboxes (`DrawFrameControl`) clearly distinguishing checked `[x]` and unchecked `[ ]` items.
6. **16-bit Color & Contrast**:
   - All text, borders, shaded code/table headers, and checkboxes maintain crisp contrast at 800×600 @ 16-bit color.
