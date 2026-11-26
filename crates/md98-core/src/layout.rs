//! Flat, arena-reusing GFM parser and layout IR generator.
//!
//! Pure integer math (`i32` / `usize`) with zero floating-point ops so it runs
//! fast and cleanly on Pentium Pro + `r9xrsrt`.

use crate::cp1252::utf8_to_cp1252_into;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// RGB color packed as Win32 `COLORREF` (`0x00BBGGRR`).
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

pub const COLOR_TEXT: u32 = rgb(0x18, 0x18, 0x18);
pub const COLOR_MUTED: u32 = rgb(0x55, 0x55, 0x55);
pub const COLOR_LINK: u32 = rgb(0x00, 0x00, 0xCC);
pub const COLOR_CODE_BG: u32 = rgb(0xF0, 0xF0, 0xF0);
pub const COLOR_INLINE_CODE_BG: u32 = rgb(0xEA, 0xEA, 0xEA);
pub const COLOR_CODE_BORDER: u32 = rgb(0xC0, 0xC0, 0xC0);
pub const COLOR_QUOTE_BAR: u32 = rgb(0x80, 0x80, 0x80);
pub const COLOR_QUOTE_BG: u32 = rgb(0xF7, 0xF7, 0xF7);
pub const COLOR_RULE: u32 = rgb(0xB8, 0xB8, 0xB8);
pub const COLOR_TABLE_HEADER_BG: u32 = rgb(0xE4, 0xE8, 0xF0);
pub const COLOR_TABLE_STRIPE_BG: u32 = rgb(0xF9, 0xF9, 0xFB);
pub const COLOR_TABLE_BORDER: u32 = rgb(0xA0, 0xA0, 0xA0);

/// Font style descriptor mapped to cached Win32 `HFONT` handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontStyle {
    /// 0 = body, 1..=6 = H1..H6
    pub heading: u8,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub monospace: bool,
    pub link: bool,
}

/// Text measurement abstraction implemented by Win32 `GetTextExtentExPointA`
/// on Windows 98 and by deterministic mock measurers in host unit tests.
pub trait TextMeasurer {
    fn measure(&self, text: &str, style: FontStyle) -> (i32, i32);
}

/// Semantic kind of a laid-out block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    CodeBlock,
    BlockQuote,
    ListItem,
    TaskItem { checked: bool },
    Table,
    Rule,
}

/// Positioned block metadata for scroll sync & viewport culling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutBlock {
    pub kind: BlockKind,
    pub y_start: i32,
    pub y_end: i32,
    pub op_start: usize,
    pub op_end: usize,
    pub source_start: usize,
    pub source_end: usize,
}

/// Flat drawing primitive consumed by the double-buffered Win32 GDI renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawOp {
    FillRect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        color: u32,
    },
    StrokeRect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        color: u32,
        width: i32,
    },
    Line {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: u32,
        width: i32,
    },
    TextRun {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        /// Byte range in `DocumentLayout::cp1252_arena` (for zero-alloc Win32 `TextOutA`).
        cp1252_offset: usize,
        cp1252_len: usize,
        /// Byte range in `DocumentLayout::utf8_arena` (for host inspection/tests).
        utf8_offset: usize,
        utf8_len: usize,
        style: FontStyle,
        color: u32,
    },
    Checkbox {
        x: i32,
        y: i32,
        size: i32,
        checked: bool,
    },
}

impl DrawOp {
    /// Bounding vertical span `[top, bottom)` for fast viewport culling.
    pub fn y_bounds(&self) -> (i32, i32) {
        match *self {
            DrawOp::FillRect { y, h, .. } | DrawOp::StrokeRect { y, h, .. } => (y, y + h),
            DrawOp::Line { y1, y2, width, .. } => (y1.min(y2), y1.max(y2) + width.max(1)),
            DrawOp::TextRun { y, h, .. } => (y, y + h),
            DrawOp::Checkbox { y, size, .. } => (y, y + size),
        }
    }
}

/// Reusable layout buffer kept alive across edits to avoid heap fragmentation on 40 MB RAM.
#[derive(Debug, Default)]
pub struct DocumentLayout {
    pub blocks: Vec<LayoutBlock>,
    pub ops: Vec<DrawOp>,
    pub cp1252_arena: Vec<u8>,
    pub utf8_arena: String,
    pub content_width: i32,
    pub content_height: i32,
    pub source_len: usize,
}

impl DocumentLayout {
    pub fn new() -> Self {
        Self {
            blocks: Vec::with_capacity(128),
            ops: Vec::with_capacity(512),
            cp1252_arena: Vec::with_capacity(4096),
            utf8_arena: String::with_capacity(4096),
            content_width: 0,
            content_height: 0,
            source_len: 0,
        }
    }

    /// Clear all buffers while retaining allocated capacity.
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.ops.clear();
        self.cp1252_arena.clear();
        self.utf8_arena.clear();
        self.content_width = 0;
        self.content_height = 0;
        self.source_len = 0;
    }

    pub fn text_utf8(&self, offset: usize, len: usize) -> &str {
        &self.utf8_arena[offset..offset + len]
    }

    pub fn text_cp1252(&self, offset: usize, len: usize) -> &[u8] {
        &self.cp1252_arena[offset..offset + len]
    }

    /// Map a normalized scroll fraction (`num / den`) or source byte offset to preview Y.
    pub fn y_for_source_offset(&self, byte_offset: usize) -> i32 {
        if self.blocks.is_empty() || self.source_len == 0 {
            return 0;
        }
        for b in &self.blocks {
            if byte_offset <= b.source_end {
                return b.y_start;
            }
        }
        self.content_height
    }

    fn push_text(&mut self, s: &str) -> (usize, usize, usize, usize) {
        let utf8_offset = self.utf8_arena.len();
        self.utf8_arena.push_str(s);
        let utf8_len = s.len();

        let cp1252_offset = self.cp1252_arena.len();
        utf8_to_cp1252_into(s, &mut self.cp1252_arena);
        let cp1252_len = self.cp1252_arena.len() - cp1252_offset;

        (cp1252_offset, cp1252_len, utf8_offset, utf8_len)
    }
}

/// Inline span collected before wrapping a paragraph / heading / list item / table cell.
#[derive(Debug, Clone)]
struct InlineSpan {
    text: String,
    style: FontStyle,
    color: u32,
    hard_break: bool,
}

#[derive(Debug, Clone)]
struct ListState {
    ordered: bool,
    next_index: u64,
}

#[derive(Debug, Clone)]
struct TableState {
    alignments: Vec<Alignment>,
    header_rows: Vec<Vec<Vec<InlineSpan>>>,
    body_rows: Vec<Vec<Vec<InlineSpan>>>,
    in_head: bool,
    current_row: Vec<Vec<InlineSpan>>,
    current_cell: Vec<InlineSpan>,
    in_cell: bool,
    source_start: usize,
}

/// Layout configuration (pixels, tuned for 800x600 Win98 SE split view).
#[derive(Debug, Clone, Copy)]
pub struct LayoutConfig {
    pub margin_x: i32,
    pub margin_y: i32,
    pub paragraph_gap: i32,
    pub line_spacing: i32,
    pub list_indent: i32,
    pub quote_indent: i32,
    pub code_pad_x: i32,
    pub code_pad_y: i32,
    pub table_cell_pad_x: i32,
    pub table_cell_pad_y: i32,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            margin_x: 14,
            margin_y: 12,
            paragraph_gap: 10,
            line_spacing: 4,
            list_indent: 20,
            quote_indent: 14,
            code_pad_x: 8,
            code_pad_y: 6,
            table_cell_pad_x: 6,
            table_cell_pad_y: 4,
        }
    }
}

/// Parse GFM Markdown and populate `out` (reusing its existing heap buffers).
pub fn layout_markdown<M: TextMeasurer>(
    markdown: &str,
    viewport_width: i32,
    measurer: &M,
    out: &mut DocumentLayout,
) {
    layout_markdown_with_config(markdown, viewport_width, LayoutConfig::default(), measurer, out);
}

pub fn layout_markdown_with_config<M: TextMeasurer>(
    markdown: &str,
    viewport_width: i32,
    cfg: LayoutConfig,
    measurer: &M,
    out: &mut DocumentLayout,
) {
    out.clear();
    out.source_len = markdown.len();
    let vp_w = viewport_width.max(120);
    out.content_width = vp_w;

    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION;

    let parser = Parser::new_ext(markdown, options).into_offset_iter();

    let mut y = cfg.margin_y;
    let mut style = FontStyle::default();
    let mut quote_depth: i32 = 0;
    let mut quote_y_stack: Vec<(i32, usize)> = Vec::new();
    let mut list_stack: Vec<ListState> = Vec::new();

    let mut in_code_block = false;
    let mut code_buf = String::new();
    let mut code_source_start = 0usize;

    let mut pending_spans: Vec<InlineSpan> = Vec::new();
    let mut pending_block_kind: Option<BlockKind> = None;
    let mut pending_source_start: usize = 0;
    let mut pending_list_marker: Option<String> = None;
    let mut pending_task_checkbox: Option<bool> = None;

    let mut table_state: Option<TableState> = None;

    let content_left = |q_depth: i32, l_depth: usize| -> i32 {
        cfg.margin_x + q_depth * cfg.quote_indent + (l_depth as i32) * cfg.list_indent
    };

    let flush_inline_block = |out: &mut DocumentLayout,
                              y: &mut i32,
                              spans: &mut Vec<InlineSpan>,
                              kind: BlockKind,
                              q_depth: i32,
                              l_depth: usize,
                              list_marker: &mut Option<String>,
                              task_checkbox: &mut Option<bool>,
                              src_start: usize,
                              src_end: usize| {
        if spans.is_empty() && list_marker.is_none() && task_checkbox.is_none() {
            return;
        }
        let left = content_left(q_depth, l_depth);
        let right = (vp_w - cfg.margin_x).max(left + 60);
        let avail_w = (right - left).max(40);

        let block_y_start = *y;
        let op_start = out.ops.len();

        // Draw list bullet/number or task checkbox in the gutter left of `left`
        if let Some(checked) = task_checkbox.take() {
            let box_size = 13;
            let bx = (left - 18).max(cfg.margin_x);
            let (_, line_h) = measurer.measure("Ag", FontStyle::default());
            let by = *y + ((line_h - box_size).max(0) / 2);
            out.ops.push(DrawOp::Checkbox {
                x: bx,
                y: by,
                size: box_size,
                checked,
            });
            *list_marker = None;
        } else if let Some(marker) = list_marker.take() {
            let m_style = FontStyle::default();
            let (mw, mh) = measurer.measure(&marker, m_style);
            let mx = (left - mw - 6).max(cfg.margin_x);
            let (co, cl, uo, ul) = out.push_text(&marker);
            out.ops.push(DrawOp::TextRun {
                x: mx,
                y: *y,
                w: mw,
                h: mh,
                cp1252_offset: co,
                cp1252_len: cl,
                utf8_offset: uo,
                utf8_len: ul,
                style: m_style,
                color: COLOR_TEXT,
            });
        }

        let end_y = wrap_and_emit_spans(
            out,
            measurer,
            spans,
            left,
            *y,
            avail_w,
            Alignment::Left,
            cfg.line_spacing,
        );
        *y = end_y;

        // Add subtle bottom rule under H1 and H2
        if let BlockKind::Heading(lvl) = kind {
            if lvl <= 2 {
                *y += 3;
                out.ops.push(DrawOp::Line {
                    x1: left,
                    y1: *y,
                    x2: right,
                    y2: *y,
                    color: COLOR_RULE,
                    width: 1,
                });
                *y += 4;
            }
        }

        let op_end = out.ops.len();
        out.blocks.push(LayoutBlock {
            kind,
            y_start: block_y_start,
            y_end: *y,
            op_start,
            op_end,
            source_start: src_start,
            source_end: src_end,
        });

        let gap = match kind {
            BlockKind::ListItem | BlockKind::TaskItem { .. } => 4,
            BlockKind::Heading(1) => cfg.paragraph_gap + 2,
            _ => cfg.paragraph_gap,
        };
        *y += gap;
        spans.clear();
    };

    for (event, range) in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    if !pending_spans.is_empty() {
                        let k = pending_block_kind.unwrap_or(BlockKind::Paragraph);
                        flush_inline_block(
                            out,
                            &mut y,
                            &mut pending_spans,
                            k,
                            quote_depth,
                            list_stack.len(),
                            &mut pending_list_marker,
                            &mut pending_task_checkbox,
                            pending_source_start,
                            range.start,
                        );
                    }
                    let lvl = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    if y > cfg.margin_y {
                        y += 4;
                    }
                    style.heading = lvl;
                    style.bold = true;
                    pending_block_kind = Some(BlockKind::Heading(lvl));
                    pending_source_start = range.start;
                }
                Tag::Paragraph => {
                    if pending_block_kind.is_none() {
                        pending_block_kind = Some(BlockKind::Paragraph);
                        pending_source_start = range.start;
                    }
                }
                Tag::BlockQuote(_) => {
                    if !pending_spans.is_empty() {
                        let k = pending_block_kind.unwrap_or(BlockKind::Paragraph);
                        flush_inline_block(
                            out,
                            &mut y,
                            &mut pending_spans,
                            k,
                            quote_depth,
                            list_stack.len(),
                            &mut pending_list_marker,
                            &mut pending_task_checkbox,
                            pending_source_start,
                            range.start,
                        );
                        pending_block_kind = None;
                    }
                    quote_depth += 1;
                    quote_y_stack.push((y, range.start));
                }
                Tag::CodeBlock(_kind @ (CodeBlockKind::Fenced(_) | CodeBlockKind::Indented)) => {
                    if !pending_spans.is_empty() {
                        let k = pending_block_kind.unwrap_or(BlockKind::Paragraph);
                        flush_inline_block(
                            out,
                            &mut y,
                            &mut pending_spans,
                            k,
                            quote_depth,
                            list_stack.len(),
                            &mut pending_list_marker,
                            &mut pending_task_checkbox,
                            pending_source_start,
                            range.start,
                        );
                        pending_block_kind = None;
                    }
                    in_code_block = true;
                    code_buf.clear();
                    code_source_start = range.start;
                }
                Tag::List(start_num) => {
                    if !pending_spans.is_empty() {
                        let k = pending_block_kind.unwrap_or(BlockKind::ListItem);
                        flush_inline_block(
                            out,
                            &mut y,
                            &mut pending_spans,
                            k,
                            quote_depth,
                            list_stack.len(),
                            &mut pending_list_marker,
                            &mut pending_task_checkbox,
                            pending_source_start,
                            range.start,
                        );
                        pending_block_kind = None;
                    }
                    list_stack.push(ListState {
                        ordered: start_num.is_some(),
                        next_index: start_num.unwrap_or(1),
                    });
                }
                Tag::Item => {
                    if !pending_spans.is_empty() {
                        let k = pending_block_kind.unwrap_or(BlockKind::ListItem);
                        flush_inline_block(
                            out,
                            &mut y,
                            &mut pending_spans,
                            k,
                            quote_depth,
                            list_stack.len(),
                            &mut pending_list_marker,
                            &mut pending_task_checkbox,
                            pending_source_start,
                            range.start,
                        );
                    }
                    let depth = list_stack.len();
                    let marker = if let Some(top) = list_stack.last_mut() {
                        if top.ordered {
                            let idx = top.next_index;
                            top.next_index += 1;
                            format!("{}.", idx)
                        } else {
                            match depth {
                                1 => "\u{2022}".to_string(), // •
                                2 => "-".to_string(),
                                _ => "*".to_string(),
                            }
                        }
                    } else {
                        "\u{2022}".to_string()
                    };
                    pending_list_marker = Some(marker);
                    pending_task_checkbox = None;
                    pending_block_kind = Some(BlockKind::ListItem);
                    pending_source_start = range.start;
                }
                Tag::Emphasis => style.italic = true,
                Tag::Strong => style.bold = true,
                Tag::Strikethrough => style.strikethrough = true,
                Tag::Link { .. } => style.link = true,
                Tag::Table(alignments) => {
                    if !pending_spans.is_empty() {
                        let k = pending_block_kind.unwrap_or(BlockKind::Paragraph);
                        flush_inline_block(
                            out,
                            &mut y,
                            &mut pending_spans,
                            k,
                            quote_depth,
                            list_stack.len(),
                            &mut pending_list_marker,
                            &mut pending_task_checkbox,
                            pending_source_start,
                            range.start,
                        );
                        pending_block_kind = None;
                    }
                    table_state = Some(TableState {
                        alignments,
                        header_rows: Vec::new(),
                        body_rows: Vec::new(),
                        in_head: false,
                        current_row: Vec::new(),
                        current_cell: Vec::new(),
                        in_cell: false,
                        source_start: range.start,
                    });
                }
                Tag::TableHead => {
                    if let Some(ts) = table_state.as_mut() {
                        ts.in_head = true;
                        ts.current_row.clear();
                    }
                }
                Tag::TableRow => {
                    if let Some(ts) = table_state.as_mut() {
                        ts.current_row.clear();
                    }
                }
                Tag::TableCell => {
                    if let Some(ts) = table_state.as_mut() {
                        ts.current_cell.clear();
                        ts.in_cell = true;
                    }
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    let k = pending_block_kind.unwrap_or(BlockKind::Heading(style.heading.max(1)));
                    flush_inline_block(
                        out,
                        &mut y,
                        &mut pending_spans,
                        k,
                        quote_depth,
                        list_stack.len(),
                        &mut pending_list_marker,
                        &mut pending_task_checkbox,
                        pending_source_start,
                        range.end,
                    );
                    style.heading = 0;
                    style.bold = false;
                    pending_block_kind = None;
                }
                TagEnd::Paragraph => {
                    let k = pending_block_kind.unwrap_or(BlockKind::Paragraph);
                    flush_inline_block(
                        out,
                        &mut y,
                        &mut pending_spans,
                        k,
                        quote_depth,
                        list_stack.len(),
                        &mut pending_list_marker,
                        &mut pending_task_checkbox,
                        pending_source_start,
                        range.end,
                    );
                    pending_block_kind = None;
                }
                TagEnd::BlockQuote(_) => {
                    if !pending_spans.is_empty() {
                        let k = pending_block_kind.unwrap_or(BlockKind::Paragraph);
                        flush_inline_block(
                            out,
                            &mut y,
                            &mut pending_spans,
                            k,
                            quote_depth,
                            list_stack.len(),
                            &mut pending_list_marker,
                            &mut pending_task_checkbox,
                            pending_source_start,
                            range.end,
                        );
                        pending_block_kind = None;
                    }
                    if let Some((qy_start, q_src)) = quote_y_stack.pop() {
                        let bar_x = cfg.margin_x + (quote_depth - 1) * cfg.quote_indent + 2;
                        let qy_end = (y - 4).max(qy_start + 12);
                        let op_start = out.ops.len();
                        out.ops.push(DrawOp::FillRect {
                            x: bar_x,
                            y: qy_start,
                            w: 3,
                            h: qy_end - qy_start,
                            color: COLOR_QUOTE_BAR,
                        });
                        out.blocks.push(LayoutBlock {
                            kind: BlockKind::BlockQuote,
                            y_start: qy_start,
                            y_end: qy_end,
                            op_start,
                            op_end: out.ops.len(),
                            source_start: q_src,
                            source_end: range.end,
                        });
                    }
                    quote_depth = (quote_depth - 1).max(0);
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    let left = content_left(quote_depth, list_stack.len());
                    let right = (vp_w - cfg.margin_x).max(left + 60);
                    let box_w = (right - left).max(60);
                    let code_style = FontStyle {
                        monospace: true,
                        ..FontStyle::default()
                    };
                    let (_, ch) = measurer.measure("Mg", code_style);
                    let line_h = ch.max(12) + 2;

                    let trimmed = code_buf.trim_end_matches('\n');
                    let lines: Vec<&str> = if trimmed.is_empty() {
                        vec![""]
                    } else {
                        trimmed.split('\n').collect()
                    };

                    // Wrap long code lines if needed so nothing clips at viewport right edge
                    let inner_w = (box_w - cfg.code_pad_x * 2).max(40);
                    let mut wrapped_lines: Vec<String> = Vec::with_capacity(lines.len());
                    for raw_line in lines {
                        if raw_line.is_empty() {
                            wrapped_lines.push(String::new());
                        } else {
                            wrap_code_line(raw_line, inner_w, code_style, measurer, &mut wrapped_lines);
                        }
                    }

                    let block_y_start = y;
                    let op_start = out.ops.len();
                    let box_h = cfg.code_pad_y * 2 + (wrapped_lines.len() as i32) * line_h;

                    out.ops.push(DrawOp::FillRect {
                        x: left,
                        y,
                        w: box_w,
                        h: box_h,
                        color: COLOR_CODE_BG,
                    });
                    out.ops.push(DrawOp::StrokeRect {
                        x: left,
                        y,
                        w: box_w,
                        h: box_h,
                        color: COLOR_CODE_BORDER,
                        width: 1,
                    });

                    let mut cy = y + cfg.code_pad_y;
                    for wl in &wrapped_lines {
                        if !wl.is_empty() {
                            let (tw, th) = measurer.measure(wl, code_style);
                            let (co, cl, uo, ul) = out.push_text(wl);
                            out.ops.push(DrawOp::TextRun {
                                x: left + cfg.code_pad_x,
                                y: cy,
                                w: tw,
                                h: th,
                                cp1252_offset: co,
                                cp1252_len: cl,
                                utf8_offset: uo,
                                utf8_len: ul,
                                style: code_style,
                                color: COLOR_TEXT,
                            });
                        }
                        cy += line_h;
                    }

                    y += box_h;
                    out.blocks.push(LayoutBlock {
                        kind: BlockKind::CodeBlock,
                        y_start: block_y_start,
                        y_end: y,
                        op_start,
                        op_end: out.ops.len(),
                        source_start: code_source_start,
                        source_end: range.end,
                    });
                    y += cfg.paragraph_gap;
                    code_buf.clear();
                }
                TagEnd::List(_) => {
                    if !pending_spans.is_empty() {
                        let k = pending_block_kind.unwrap_or(BlockKind::ListItem);
                        flush_inline_block(
                            out,
                            &mut y,
                            &mut pending_spans,
                            k,
                            quote_depth,
                            list_stack.len(),
                            &mut pending_list_marker,
                            &mut pending_task_checkbox,
                            pending_source_start,
                            range.end,
                        );
                        pending_block_kind = None;
                    }
                    list_stack.pop();
                    if list_stack.is_empty() {
                        y += 4;
                    }
                }
                TagEnd::Item => {
                    let k = pending_block_kind.unwrap_or(BlockKind::ListItem);
                    flush_inline_block(
                        out,
                        &mut y,
                        &mut pending_spans,
                        k,
                        quote_depth,
                        list_stack.len(),
                        &mut pending_list_marker,
                        &mut pending_task_checkbox,
                        pending_source_start,
                        range.end,
                    );
                    pending_block_kind = None;
                }
                TagEnd::Emphasis => style.italic = false,
                TagEnd::Strong => style.bold = false,
                TagEnd::Strikethrough => style.strikethrough = false,
                TagEnd::Link => style.link = false,
                TagEnd::TableCell => {
                    if let Some(ts) = table_state.as_mut() {
                        let cell = core::mem::take(&mut ts.current_cell);
                        ts.current_row.push(cell);
                        ts.in_cell = false;
                    }
                }
                TagEnd::TableHead => {
                    if let Some(ts) = table_state.as_mut() {
                        if !ts.current_row.is_empty() {
                            let row = core::mem::take(&mut ts.current_row);
                            ts.header_rows.push(row);
                        }
                        ts.in_head = false;
                    }
                }
                TagEnd::TableRow => {
                    if let Some(ts) = table_state.as_mut() {
                        if !ts.current_row.is_empty() {
                            let row = core::mem::take(&mut ts.current_row);
                            if ts.in_head {
                                ts.header_rows.push(row);
                            } else {
                                ts.body_rows.push(row);
                            }
                        }
                    }
                }
                TagEnd::Table => {
                    if let Some(ts) = table_state.take() {
                        let left = content_left(quote_depth, list_stack.len());
                        let right = (vp_w - cfg.margin_x).max(left + 80);
                        y = layout_table(
                            out,
                            measurer,
                            &ts,
                            left,
                            y,
                            right - left,
                            cfg,
                            range.end,
                        );
                        y += cfg.paragraph_gap;
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                if in_code_block {
                    code_buf.push_str(&t);
                } else {
                    let base_color = if style.link {
                        COLOR_LINK
                    } else if quote_depth > 0 {
                        COLOR_MUTED
                    } else {
                        COLOR_TEXT
                    };
                    let mut pieces: Vec<InlineSpan> = Vec::new();
                    if style.link || style.monospace {
                        pieces.push(InlineSpan {
                            text: t.to_string(),
                            style,
                            color: base_color,
                            hard_break: false,
                        });
                    } else {
                        split_autolinks(&t, style, base_color, &mut pieces);
                    }
                    if let Some(ts) = table_state.as_mut() {
                        if ts.in_cell {
                            for mut sp in pieces {
                                if ts.in_head {
                                    sp.style.bold = true;
                                }
                                push_merged_span(&mut ts.current_cell, sp);
                            }
                            continue;
                        }
                    }
                    for sp in pieces {
                        push_merged_span(&mut pending_spans, sp);
                    }
                }
            }
            Event::Code(c) => {
                let mut cs = style;
                cs.monospace = true;
                let span = InlineSpan {
                    text: c.to_string(),
                    style: cs,
                    color: if style.link { COLOR_LINK } else { COLOR_TEXT },
                    hard_break: false,
                };
                if let Some(ts) = table_state.as_mut() {
                    if ts.in_cell {
                        let mut cell_style = span.style;
                        if ts.in_head {
                            cell_style.bold = true;
                        }
                        ts.current_cell.push(InlineSpan {
                            style: cell_style,
                            ..span
                        });
                        continue;
                    }
                }
                pending_spans.push(span);
            }
            Event::SoftBreak => {
                if in_code_block {
                    code_buf.push('\n');
                } else {
                    let span = InlineSpan {
                        text: " ".to_string(),
                        style,
                        color: COLOR_TEXT,
                        hard_break: false,
                    };
                    if let Some(ts) = table_state.as_mut() {
                        if ts.in_cell {
                            ts.current_cell.push(span);
                            continue;
                        }
                    }
                    pending_spans.push(span);
                }
            }
            Event::HardBreak => {
                if in_code_block {
                    code_buf.push('\n');
                } else {
                    let span = InlineSpan {
                        text: String::new(),
                        style,
                        color: COLOR_TEXT,
                        hard_break: true,
                    };
                    if let Some(ts) = table_state.as_mut() {
                        if ts.in_cell {
                            ts.current_cell.push(span);
                            continue;
                        }
                    }
                    pending_spans.push(span);
                }
            }
            Event::Rule => {
                if !pending_spans.is_empty() {
                    let k = pending_block_kind.unwrap_or(BlockKind::Paragraph);
                    flush_inline_block(
                        out,
                        &mut y,
                        &mut pending_spans,
                        k,
                        quote_depth,
                        list_stack.len(),
                        &mut pending_list_marker,
                        &mut pending_task_checkbox,
                        pending_source_start,
                        range.start,
                    );
                    pending_block_kind = None;
                }
                let left = content_left(quote_depth, list_stack.len());
                let right = (vp_w - cfg.margin_x).max(left + 40);
                let op_start = out.ops.len();
                let ry = y + 4;
                out.ops.push(DrawOp::Line {
                    x1: left,
                    y1: ry,
                    x2: right,
                    y2: ry,
                    color: COLOR_RULE,
                    width: 2,
                });
                out.blocks.push(LayoutBlock {
                    kind: BlockKind::Rule,
                    y_start: y,
                    y_end: ry + 6,
                    op_start,
                    op_end: out.ops.len(),
                    source_start: range.start,
                    source_end: range.end,
                });
                y = ry + 6 + cfg.paragraph_gap;
            }
            Event::TaskListMarker(checked) => {
                pending_task_checkbox = Some(checked);
                pending_block_kind = Some(BlockKind::TaskItem { checked });
            }
            _ => {}
        }
    }

    if !pending_spans.is_empty() {
        let k = pending_block_kind.unwrap_or(BlockKind::Paragraph);
        flush_inline_block(
            out,
            &mut y,
            &mut pending_spans,
            k,
            quote_depth,
            list_stack.len(),
            &mut pending_list_marker,
            &mut pending_task_checkbox,
            pending_source_start,
            markdown.len(),
        );
    }

    out.content_height = (y + cfg.margin_y).max(cfg.margin_y * 2);
}

#[derive(Debug, Clone)]
struct PlacedRun {
    text: String,
    w: i32,
    h: i32,
    style: FontStyle,
    color: u32,
    is_space: bool,
}

/// Tokenize inline spans into words/spaces and wrap them into `avail_w`,
/// breaking overly long unbroken words (like URLs) character-by-character.
fn wrap_and_emit_spans<M: TextMeasurer>(
    out: &mut DocumentLayout,
    measurer: &M,
    spans: &[InlineSpan],
    x_start: i32,
    y_start: i32,
    avail_w: i32,
    align: Alignment,
    line_spacing: i32,
) -> i32 {
    let mut lines: Vec<Vec<PlacedRun>> = Vec::new();
    let mut cur_line: Vec<PlacedRun> = Vec::new();
    let mut cur_w: i32 = 0;

    let push_line = |lines: &mut Vec<Vec<PlacedRun>>, cur_line: &mut Vec<PlacedRun>, cur_w: &mut i32| {
        // Trim trailing whitespace run on wrapped line
        while cur_line.last().is_some_and(|r| r.is_space) {
            cur_line.pop();
        }
        lines.push(core::mem::take(cur_line));
        *cur_w = 0;
    };

    for span in spans {
        if span.hard_break {
            push_line(&mut lines, &mut cur_line, &mut cur_w);
            continue;
        }
        if span.text.is_empty() {
            continue;
        }

        // Split into alternating whitespace / non-whitespace segments
        let mut idx = 0usize;
        let bytes = span.text.as_bytes();
        while idx < bytes.len() {
            let is_ws = bytes[idx].is_ascii_whitespace();
            let seg_start = idx;
            while idx < bytes.len() && bytes[idx].is_ascii_whitespace() == is_ws {
                idx += 1;
            }
            let seg = &span.text[seg_start..idx];
            let token_str = if is_ws { " " } else { seg };

            // Collapse leading space at start of line
            if is_ws && cur_line.is_empty() {
                continue;
            }
            // Collapse consecutive spaces
            if is_ws && cur_line.last().is_some_and(|r| r.is_space) {
                continue;
            }

            let (tw, th) = measurer.measure(token_str, span.style);
            let eff_w = tw;

            if !is_ws && cur_w + eff_w > avail_w && !cur_line.is_empty() {
                push_line(&mut lines, &mut cur_line, &mut cur_w);
            }

            if !is_ws && eff_w > avail_w {
                // Break long unbroken word (e.g. URL) by character so it never overflows
                let mut chunk = String::new();
                for ch in token_str.chars() {
                    chunk.push(ch);
                    let (cw, ch_h) = measurer.measure(&chunk, span.style);
                    if cw > avail_w && chunk.chars().count() > 1 {
                        chunk.pop();
                        let (pw, ph) = measurer.measure(&chunk, span.style);
                        cur_line.push(PlacedRun {
                            text: core::mem::take(&mut chunk),
                            w: pw,
                            h: ph,
                            style: span.style,
                            color: span.color,
                            is_space: false,
                        });
                        push_line(&mut lines, &mut cur_line, &mut cur_w);
                        chunk.push(ch);
                    } else {
                        let _ = ch_h;
                    }
                }
                if !chunk.is_empty() {
                    let (pw, ph) = measurer.measure(&chunk, span.style);
                    cur_w += pw;
                    cur_line.push(PlacedRun {
                        text: chunk,
                        w: pw,
                        h: ph,
                        style: span.style,
                        color: span.color,
                        is_space: false,
                    });
                }
            } else {
                cur_w += eff_w;
                cur_line.push(PlacedRun {
                    text: token_str.to_string(),
                    w: eff_w,
                    h: th,
                    style: span.style,
                    color: span.color,
                    is_space: is_ws,
                });
            }
        }
    }

    if !cur_line.is_empty() {
        push_line(&mut lines, &mut cur_line, &mut cur_w);
    }

    if lines.is_empty() {
        let (_, default_h) = measurer.measure("Ag", FontStyle::default());
        return y_start + default_h.max(12);
    }

    let mut y = y_start;
    for line in &lines {
        let line_w: i32 = line.iter().map(|r| r.w).sum();
        let line_h: i32 = line.iter().map(|r| r.h).max().unwrap_or(14).max(12);

        let mut x = match align {
            Alignment::Center => x_start + ((avail_w - line_w).max(0) / 2),
            Alignment::Right => x_start + (avail_w - line_w).max(0),
            Alignment::Left | Alignment::None => x_start,
        };

        // Coalesce adjacent runs that share identical style & color
        // (including contiguous monospace spans so inline code forms ONE pill!).
        let mut i = 0;
        while i < line.len() {
            let run = &line[i];
            let mut combined = run.text.clone();
            let mut combined_w = run.w;
            let run_h = run.h;
            let run_style = run.style;
            let run_color = run.color;
            i += 1;
            while i < line.len() {
                let next = &line[i];
                if next.style == run_style && next.color == run_color && next.h == run_h {
                    combined.push_str(&next.text);
                    combined_w += next.w;
                    i += 1;
                } else {
                    break;
                }
            }
            let ry = y + (line_h - run_h);
            if run_style.monospace && !combined.trim().is_empty() {
                out.ops.push(DrawOp::FillRect {
                    x: x - 1,
                    y: ry,
                    w: combined_w + 2,
                    h: run_h,
                    color: COLOR_INLINE_CODE_BG,
                });
            }
            let (co, cl, uo, ul) = out.push_text(&combined);
            out.ops.push(DrawOp::TextRun {
                x,
                y: ry,
                w: combined_w,
                h: run_h,
                cp1252_offset: co,
                cp1252_len: cl,
                utf8_offset: uo,
                utf8_len: ul,
                style: run_style,
                color: run_color,
            });
            x += combined_w;
        }
        y += line_h + line_spacing;
    }

    y
}

fn push_merged_span(target: &mut Vec<InlineSpan>, span: InlineSpan) {
    if !span.hard_break {
        if let Some(last) = target.last_mut() {
            if !last.hard_break && last.style == span.style && last.color == span.color {
                last.text.push_str(&span.text);
                return;
            }
        }
    }
    target.push(span);
}

/// Detect GFM bare URLs (`http://...` and `https://...`) inside plain text spans.
fn split_autolinks(text: &str, style: FontStyle, base_color: u32, out: &mut Vec<InlineSpan>) {
    let mut rest = text;
    while !rest.is_empty() {
        let next_http = rest.find("http://");
        let next_https = rest.find("https://");
        let start = match (next_http, next_https) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => {
                out.push(InlineSpan {
                    text: rest.to_string(),
                    style,
                    color: base_color,
                    hard_break: false,
                });
                return;
            }
        };
        if start > 0 {
            out.push(InlineSpan {
                text: rest[..start].to_string(),
                style,
                color: base_color,
                hard_break: false,
            });
        }
        let after = &rest[start..];
        let mut end = after.find(char::is_whitespace).unwrap_or(after.len());
        while end > 0 {
            let last_ch = after[..end].chars().next_back().unwrap_or(' ');
            if matches!(last_ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']') {
                end -= last_ch.len_utf8();
            } else {
                break;
            }
        }
        if end <= 8 {
            out.push(InlineSpan {
                text: after[..end.max(1)].to_string(),
                style,
                color: base_color,
                hard_break: false,
            });
            rest = &after[end.max(1)..];
        } else {
            let mut link_style = style;
            link_style.link = true;
            out.push(InlineSpan {
                text: after[..end].to_string(),
                style: link_style,
                color: COLOR_LINK,
                hard_break: false,
            });
            rest = &after[end..];
        }
    }
}

fn wrap_code_line<M: TextMeasurer>(
    line: &str,
    max_w: i32,
    style: FontStyle,
    measurer: &M,
    out_lines: &mut Vec<String>,
) {
    let (w, _) = measurer.measure(line, style);
    if w <= max_w {
        out_lines.push(line.to_string());
        return;
    }
    let mut cur = String::new();
    for ch in line.chars() {
        cur.push(ch);
        let (cw, _) = measurer.measure(&cur, style);
        if cw > max_w && cur.chars().count() > 1 {
            cur.pop();
            out_lines.push(core::mem::take(&mut cur));
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out_lines.push(cur);
    }
}

/// Two-pass GFM table layout:
/// Pass 1: measure min (longest word) & max (single-line) widths per column.
/// Pass 2: distribute available viewport width proportionally, wrap each cell, emit shaded header + 1px borders.
fn layout_table<M: TextMeasurer>(
    out: &mut DocumentLayout,
    measurer: &M,
    ts: &TableState,
    x_start: i32,
    y_start: i32,
    avail_w: i32,
    cfg: LayoutConfig,
    source_end: usize,
) -> i32 {
    let num_cols = ts
        .alignments
        .len()
        .max(ts.header_rows.iter().map(|r| r.len()).max().unwrap_or(0))
        .max(ts.body_rows.iter().map(|r| r.len()).max().unwrap_or(0));

    if num_cols == 0 {
        return y_start;
    }

    let pad_2x = cfg.table_cell_pad_x * 2;
    let mut min_w = vec![32i32; num_cols];
    let mut max_w = vec![48i32; num_cols];

    // Pass 1: measure min & max column content widths
    for row in ts.header_rows.iter().chain(ts.body_rows.iter()) {
        for (c, cell) in row.iter().enumerate() {
            if c >= num_cols {
                break;
            }
            let mut line_total = 0i32;
            let mut longest_word = 0i32;
            for span in cell {
                let (sw, _) = measurer.measure(&span.text, span.style);
                line_total += sw + if span.style.monospace { 4 } else { 0 };
                for word in span.text.split_whitespace() {
                    let (ww, _) = measurer.measure(word, span.style);
                    longest_word = longest_word.max(ww);
                }
            }
            min_w[c] = min_w[c].max(longest_word.min(avail_w / (num_cols as i32).max(1)) + pad_2x);
            max_w[c] = max_w[c].max(line_total + pad_2x);
        }
    }

    // Compute final column widths fitting inside `avail_w`
    let sum_max: i32 = max_w.iter().sum();
    let sum_min: i32 = min_w.iter().sum();
    let mut col_w = vec![0i32; num_cols];

    if sum_max <= avail_w {
        col_w.copy_from_slice(&max_w);
    } else if sum_min >= avail_w {
        // Scale down proportionally to fit viewport width
        let denom = sum_min.max(1);
        let mut used = 0i32;
        for c in 0..num_cols {
            let w = if c + 1 == num_cols {
                (avail_w - used).max(24)
            } else {
                ((min_w[c] as i64 * avail_w as i64) / denom as i64).max(24) as i32
            };
            col_w[c] = w;
            used += w;
        }
    } else {
        // Give each column its min width + proportional share of remaining space toward max_w
        let extra_avail = avail_w - sum_min;
        let extra_want: i32 = (0..num_cols).map(|c| (max_w[c] - min_w[c]).max(0)).sum();
        let denom = extra_want.max(1);
        let mut used = 0i32;
        for c in 0..num_cols {
            let want = (max_w[c] - min_w[c]).max(0);
            let w = if c + 1 == num_cols {
                (avail_w - used).max(min_w[c])
            } else {
                min_w[c] + ((want as i64 * extra_avail as i64) / denom as i64) as i32
            };
            col_w[c] = w;
            used += w;
        }
    }

    let table_w: i32 = col_w.iter().sum();
    let op_start = out.ops.len();
    let mut y = y_start;

    let all_rows: Vec<(bool, &Vec<Vec<InlineSpan>>)> = ts
        .header_rows
        .iter()
        .map(|r| (true, r))
        .chain(ts.body_rows.iter().map(|r| (false, r)))
        .collect();

    for (row_idx, (is_header, row)) in all_rows.iter().enumerate() {
        let row_y = y;
        // Reserve background FillRect op so text draws on top of it
        let bg_op_idx = out.ops.len();
        let bg_color = if *is_header {
            Some(COLOR_TABLE_HEADER_BG)
        } else if row_idx % 2 == 1 {
            Some(COLOR_TABLE_STRIPE_BG)
        } else {
            None
        };
        if let Some(color) = bg_color {
            out.ops.push(DrawOp::FillRect {
                x: x_start,
                y: row_y,
                w: table_w,
                h: 0, // patched after measuring wrapped cells
                color,
            });
        }

        let mut max_cell_end_y = row_y + cfg.table_cell_pad_y + 16;
        let mut cx = x_start;
        for c in 0..num_cols {
            let cw = col_w[c];
            let inner_w = (cw - pad_2x).max(16);
            let align = ts.alignments.get(c).copied().unwrap_or(Alignment::Left);
            if let Some(cell) = row.get(c) {
                if !cell.is_empty() {
                    let end_y = wrap_and_emit_spans(
                        out,
                        measurer,
                        cell,
                        cx + cfg.table_cell_pad_x,
                        row_y + cfg.table_cell_pad_y,
                        inner_w,
                        align,
                        2,
                    );
                    max_cell_end_y = max_cell_end_y.max(end_y);
                }
            }
            cx += cw;
        }

        let row_h = (max_cell_end_y - row_y) + cfg.table_cell_pad_y;
        if bg_color.is_some() {
            if let DrawOp::FillRect { h, .. } = &mut out.ops[bg_op_idx] {
                *h = row_h;
            }
        }

        // Horizontal row divider
        out.ops.push(DrawOp::Line {
            x1: x_start,
            y1: row_y + row_h,
            x2: x_start + table_w,
            y2: row_y + row_h,
            color: COLOR_TABLE_BORDER,
            width: 1,
        });

        y = row_y + row_h;
    }

    let table_h = y - y_start;
    // Outer border + internal vertical column dividers
    out.ops.push(DrawOp::StrokeRect {
        x: x_start,
        y: y_start,
        w: table_w,
        h: table_h,
        color: COLOR_TABLE_BORDER,
        width: 1,
    });
    let mut vx = x_start;
    for &cw in &col_w[..num_cols.saturating_sub(1)] {
        vx += cw;
        out.ops.push(DrawOp::Line {
            x1: vx,
            y1: y_start,
            x2: vx,
            y2: y_start + table_h,
            color: COLOR_TABLE_BORDER,
            width: 1,
        });
    }

    out.blocks.push(LayoutBlock {
        kind: BlockKind::Table,
        y_start,
        y_end: y,
        op_start,
        op_end: out.ops.len(),
        source_start: ts.source_start,
        source_end,
    });

    y
}
