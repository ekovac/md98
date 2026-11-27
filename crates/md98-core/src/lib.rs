//! Pure-Rust GFM parser, CP1252 codec, and arena-reusing layout IR for md98.

pub mod cp1252;
pub mod layout;
pub mod reflow;
pub mod search;

pub use cp1252::{cp1252_to_utf8, normalize_to_crlf, normalize_to_lf, utf8_to_cp1252, utf8_to_cp1252_into};
pub use layout::{
    layout_markdown, layout_markdown_with_config, layout_markdown_with_theme, rgb, BlockKind,
    DocumentLayout, DrawOp, FontStyle, LayoutBlock, LayoutConfig, TextMeasurer, ThemePalette,
    COLOR_CODE_BG, COLOR_CODE_BORDER, COLOR_INLINE_CODE_BG, COLOR_LINK, COLOR_MUTED,
    COLOR_QUOTE_BAR, COLOR_QUOTE_BG, COLOR_RULE, COLOR_TABLE_BORDER, COLOR_TABLE_HEADER_BG,
    COLOR_TABLE_STRIPE_BG, COLOR_TEXT,
};
pub use reflow::{compute_live_wrap_break, is_line_in_fenced_code, reflow_markdown, WRAP_WIDTHS};
pub use search::{find_cp1252, replace_all_text};

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic mock measurer for fast, repeatable host unit tests.
    struct MockMeasurer;

    impl TextMeasurer for MockMeasurer {
        fn measure(&self, text: &str, style: FontStyle) -> (i32, i32) {
            let char_w = match style.heading {
                1 => 11,
                2 => 10,
                3 => 9,
                4..=6 => 8,
                _ if style.monospace => 8,
                _ => 7,
            };
            let line_h = match style.heading {
                1 => 24,
                2 => 20,
                3 => 18,
                4..=6 => 16,
                _ => 15,
            };
            let w = (text.chars().count() as i32) * char_w;
            (w, line_h)
        }
    }

    #[test]
    fn cp1252_roundtrip_smart_punctuation() {
        let s = "Hello \u{201C}Win98\u{201D} \u{2014} \u{20AC}100 \u{2026} caf\u{00E9}";
        let bytes = utf8_to_cp1252(s);
        let back = cp1252_to_utf8(&bytes);
        assert_eq!(back, s);
    }

    #[test]
    fn cp1252_fallback_unknown_char() {
        let bytes = utf8_to_cp1252("Hi \u{1F600}!");
        assert_eq!(bytes, b"Hi ?!");
    }

    #[test]
    fn line_ending_normalization() {
        assert_eq!(normalize_to_crlf("a\nb\r\nc"), "a\r\nb\r\nc");
        assert_eq!(normalize_to_lf("a\r\nb\rc\nd"), "a\nb\nc\nd");
    }

    #[test]
    fn empty_document_layouts_cleanly() {
        let mut doc = DocumentLayout::new();
        layout_markdown("", 400, &MockMeasurer, &mut doc);
        assert!(doc.blocks.is_empty());
        assert!(doc.ops.is_empty());
        assert!(doc.content_height > 0);
    }

    #[test]
    fn long_unbroken_url_wraps_within_viewport() {
        let mut doc = DocumentLayout::new();
        let md = "https://example.com/a/very/long/unbroken/path/segment/that/exceeds/viewport/width/easily";
        layout_markdown(md, 240, &MockMeasurer, &mut doc);
        let mut text_runs = 0;
        for op in &doc.ops {
            if let DrawOp::TextRun { x, w, .. } = *op {
                text_runs += 1;
                assert!(x + w <= 240, "TextRun overflowed viewport: x={x}, w={w}");
            }
        }
        assert!(text_runs >= 2, "Expected long URL to wrap onto multiple lines");
    }

    #[test]
    fn deeply_nested_lists_indent_progressively() {
        let mut doc = DocumentLayout::new();
        let md = "- Level 1\n  - Level 2\n    - Level 3\n      - Level 4\n";
        layout_markdown(md, 400, &MockMeasurer, &mut doc);
        let xs: Vec<i32> = doc
            .ops
            .iter()
            .filter_map(|op| match *op {
                DrawOp::TextRun { x, utf8_offset, utf8_len, .. } => {
                    let s = doc.text_utf8(utf8_offset, utf8_len);
                    if s.starts_with("Level") {
                        Some(x)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();
        assert_eq!(xs.len(), 4);
        assert!(xs[0] < xs[1] && xs[1] < xs[2] && xs[2] < xs[3], "Expected increasing indent: {xs:?}");
    }

    #[test]
    fn gfm_tables_fit_within_viewport_and_have_header_shading() {
        let mut doc = DocumentLayout::new();
        let md = "| Component | Target Limit | Strategy |\n| :--- | :---: | ---: |\n| CPU | Pentium Pro No SSE | -C target-cpu=pentiumpro |\n| RAM | 40 MB Total | Flat Arena < 4 MB |\n";
        layout_markdown(md, 360, &MockMeasurer, &mut doc);
        assert!(doc.blocks.iter().any(|b| b.kind == BlockKind::Table));
        assert!(doc
            .ops
            .iter()
            .any(|op| matches!(op, DrawOp::FillRect { color, .. } if *color == COLOR_TABLE_HEADER_BG)));
        for op in &doc.ops {
            match *op {
                DrawOp::TextRun { x, w, .. } => {
                    assert!(x + w <= 360, "Table cell text overflowed viewport: x={x} w={w}");
                }
                DrawOp::StrokeRect { x, w, .. } => {
                    assert!(x + w <= 360, "Table border overflowed viewport: x={x} w={w}");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn gfm_task_lists_strikethrough_and_smart_punctuation() {
        let mut doc = DocumentLayout::new();
        let md = "- [x] Done ~~old~~ \"quoted\"\n- [ ] Pending --- test...\n";
        layout_markdown(md, 400, &MockMeasurer, &mut doc);
        let checkboxes: Vec<bool> = doc
            .ops
            .iter()
            .filter_map(|op| match *op {
                DrawOp::Checkbox { checked, .. } => Some(checked),
                _ => None,
            })
            .collect();
        assert_eq!(checkboxes, vec![true, false]);
        assert!(doc
            .ops
            .iter()
            .any(|op| matches!(op, DrawOp::TextRun { style, .. } if style.strikethrough)));
        // Smart punctuation converted to CP1252 curly quotes (0x93 / 0x94) and ellipsis (0x85)
        assert!(doc.cp1252_arena.contains(&0x93));
        assert!(doc.cp1252_arena.contains(&0x85));
    }

    #[test]
    fn buffer_reuse_across_edits_preserves_capacity() {
        let mut doc = DocumentLayout::new();
        let md = include_str!("../../../fixtures/gfm_kitchen_sink.md");
        layout_markdown(md, 380, &MockMeasurer, &mut doc);
        let cap_ops = doc.ops.capacity();
        let cap_blocks = doc.blocks.capacity();
        let cap_cp1252 = doc.cp1252_arena.capacity();
        for _ in 0..20 {
            layout_markdown(md, 380, &MockMeasurer, &mut doc);
        }
        assert_eq!(doc.ops.capacity(), cap_ops);
        assert_eq!(doc.blocks.capacity(), cap_blocks);
        assert_eq!(doc.cp1252_arena.capacity(), cap_cp1252);
    }

    #[test]
    fn bare_gfm_autolink_styled_blue_and_underlined() {
        let mut doc = DocumentLayout::new();
        layout_markdown("Visit https://github.com/rust9x/rust.", 400, &MockMeasurer, &mut doc);
        let link_runs: Vec<&str> = doc
            .ops
            .iter()
            .filter_map(|op| match *op {
                DrawOp::TextRun {
                    utf8_offset,
                    utf8_len,
                    style,
                    color,
                    ..
                } if style.link && color == COLOR_LINK => Some(doc.text_utf8(utf8_offset, utf8_len)),
                _ => None,
            })
            .collect();
        assert_eq!(link_runs, vec!["https://github.com/rust9x/rust"]);
    }

    #[test]
    fn multiword_inline_code_renders_single_pill() {
        let mut doc = DocumentLayout::new();
        layout_markdown("Run `-m 40 -cpu pentiumpro` now.", 500, &MockMeasurer, &mut doc);
        let code_pills = doc
            .ops
            .iter()
            .filter(|op| matches!(op, DrawOp::FillRect { color, .. } if *color == COLOR_INLINE_CODE_BG))
            .count();
        assert_eq!(code_pills, 1, "Expected 1 contiguous inline-code pill");
    }

    #[test]
    fn dark_theme_inverts_preview_colors() {
        let mut doc = DocumentLayout::new();
        let md = "# Title\n\nText with `code` and https://example.com\n\n| A | B |\n| - | - |\n| 1 | 2 |\n";
        layout_markdown_with_theme(
            md,
            400,
            LayoutConfig::default(),
            ThemePalette::DARK,
            &MockMeasurer,
            &mut doc,
        );
        assert!(doc
            .ops
            .iter()
            .any(|op| matches!(op, DrawOp::TextRun { color, .. } if *color == ThemePalette::DARK.text)));
        assert!(doc
            .ops
            .iter()
            .any(|op| matches!(op, DrawOp::TextRun { color, style, .. } if style.link && *color == ThemePalette::DARK.link)));
        assert!(doc
            .ops
            .iter()
            .any(|op| matches!(op, DrawOp::FillRect { color, .. } if *color == ThemePalette::DARK.table_header_bg)));
    }

    #[test]
    fn source_offset_and_y_roundtrip_for_scroll_preservation() {
        let mut doc = DocumentLayout::new();
        let md = include_str!("../../../fixtures/gfm_kitchen_sink.md");
        layout_markdown(md, 380, &MockMeasurer, &mut doc);
        let mid_y = doc.content_height / 2;
        let src = doc.source_offset_at_y(mid_y);
        assert!(src > 0 && src < md.len());
        let mapped_y = doc.y_for_source_offset(src);
        assert!(mapped_y >= 0 && mapped_y <= doc.content_height);
    }

    #[test]
    fn reflow_wraps_paragraphs_lists_quotes_and_preserves_code_and_tables() {
        let md = "\
# Long Heading That Should Never Be Reflowed Even If It Exceeds Seventy Two Characters Wide

This is a very long paragraph written on a single line that definitely exceeds seventy two characters and should be wrapped cleanly at word boundaries.

> Blockquote line that is also very long and needs to preserve the quote prefix on every single wrapped continuation line.

- [x] Task list item with a long description that should wrap cleanly with a two-space hanging indent underneath.

| Column One | Column Two | Column Three Which Is Very Wide And Must Not Be Broken |
| :--- | :---: | ---: |
| Value 1 | Value 2 | Value 3 |

```rust
let very_long_code_line_that_must_remain_completely_untouched_during_reflow = 1234567890;
```
";
        let out72 = reflow_markdown(md, 72);
        let lines: Vec<&str> = out72.lines().collect();
        // Heading untouched
        assert!(lines[0].starts_with("# Long Heading") && lines[0].len() > 72);
        // Paragraph lines <= 72
        let para_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("This is") || l.starts_with("characters") || l.starts_with("boundaries")).collect();
        assert!(para_lines.len() >= 2);
        for l in &para_lines {
            assert!(l.chars().count() <= 72, "Paragraph line exceeded 72: {l}");
        }
        // Blockquote preserves "> " prefix on continuation lines
        let q_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("> ")).collect();
        assert!(q_lines.len() >= 2);
        for l in &q_lines {
            assert!(l.chars().count() <= 72, "Quote line exceeded 72: {l}");
        }
        // Task list item wraps with hanging indent
        assert!(out72.contains("\n  "));
        // Table & fenced code preserved verbatim
        assert!(out72.contains("| Column One | Column Two | Column Three Which Is Very Wide And Must Not Be Broken |"));
        assert!(out72.contains("let very_long_code_line_that_must_remain_completely_untouched_during_reflow = 1234567890;"));

        // 80 and 100 column reflows also respect their widths
        let out100 = reflow_markdown("word ".repeat(30).trim(), 100);
        for l in out100.lines() {
            assert!(l.chars().count() <= 100);
        }
    }

    #[test]
    fn live_wrap_breaks_regular_and_prefixed_lines_and_skips_code() {
        let long_line = "This is a live typed sentence that just crossed the seventy two column mark cleanly";
        let (idx, ins) = compute_live_wrap_break(long_line, 72, false).expect("should wrap");
        assert!(idx <= 72);
        assert_eq!(ins, "\r\n");

        let quote_line = "> This is a live typed blockquote sentence that just crossed seventy two chars";
        let (qidx, qins) = compute_live_wrap_break(quote_line, 72, false).expect("should wrap quote");
        assert!(qidx <= 72);
        assert_eq!(qins, "\r\n> ");

        assert!(compute_live_wrap_break(long_line, 72, true).is_none());
        assert!(compute_live_wrap_break("# Heading longer than seventy two characters should not live wrap at all here", 72, false).is_none());
        assert!(is_line_in_fenced_code("```rust\nlet x = 1;\n", 1));
        assert!(!is_line_in_fenced_code("```rust\nlet x = 1;\n```\nhello\n", 3));
    }

    #[test]
    fn find_and_replace_case_whole_word_and_direction() {
        let text = "Hello world, hello Win98! Hellos are nice.";
        let hay = utf8_to_cp1252(text);
        // Case-insensitive forward search
        let m1 = find_cp1252(&hay, b"hello", 0, true, false, false).unwrap();
        assert_eq!(&hay[m1.0..m1.1], b"Hello");
        let m2 = find_cp1252(&hay, b"hello", m1.1, true, false, false).unwrap();
        assert_eq!(&hay[m2.0..m2.1], b"hello");
        // Whole word skips "Hellos"
        let m3 = find_cp1252(&hay, b"hello", m2.1, true, false, true).unwrap();
        assert_eq!(m3, m1, "Should wrap around to first whole-word match");
        // Backward search
        let mb = find_cp1252(&hay, b"hello", m2.0, false, false, true).unwrap();
        assert_eq!(mb, m1);
        // Replace All (whole-word, case-insensitive)
        let (replaced, count) = replace_all_text(text, "hello", "hi", false, true);
        assert_eq!(count, 2);
        assert_eq!(replaced, "hi world, hi Win98! Hellos are nice.");
    }

    #[test]
    fn kitchen_sink_and_basic_fixtures_layout_cleanly() {
        let mut doc = DocumentLayout::new();
        for md in [
            include_str!("../../../fixtures/basic_markdown.md"),
            include_str!("../../../fixtures/gfm_kitchen_sink.md"),
        ] {
            layout_markdown(md, 380, &MockMeasurer, &mut doc);
            assert!(!doc.blocks.is_empty());
            assert!(!doc.ops.is_empty());
            assert!(doc.content_height > 200);
        }
    }
}
