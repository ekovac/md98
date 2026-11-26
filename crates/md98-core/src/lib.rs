//! Pure-Rust GFM parser, CP1252 codec, and arena-reusing layout IR for md98.

pub mod cp1252;
pub mod layout;

pub use cp1252::{cp1252_to_utf8, normalize_to_crlf, normalize_to_lf, utf8_to_cp1252, utf8_to_cp1252_into};
pub use layout::{
    layout_markdown, layout_markdown_with_config, layout_markdown_with_theme, rgb, BlockKind,
    DocumentLayout, DrawOp, FontStyle, LayoutBlock, LayoutConfig, TextMeasurer, ThemePalette,
    COLOR_CODE_BG, COLOR_CODE_BORDER, COLOR_INLINE_CODE_BG, COLOR_LINK, COLOR_MUTED,
    COLOR_QUOTE_BAR, COLOR_QUOTE_BG, COLOR_RULE, COLOR_TABLE_BORDER, COLOR_TABLE_HEADER_BG,
    COLOR_TABLE_STRIPE_BG, COLOR_TEXT,
};

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
