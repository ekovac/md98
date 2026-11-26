//! Cached GDI resources, `GetTextExtentExPointA` measurer, and viewport-only double-buffered renderer.
//!
//! Strictly enforces zero per-frame GDI allocations to protect Windows 98 SE's 16-bit GDI heap
//! and 40 MB RAM ceiling.

#![cfg(windows)]

use crate::win32_ffi::*;
use core::cell::RefCell;
use core::ptr;
use md98_core::{
    rgb, utf8_to_cp1252_into, DocumentLayout, DrawOp, FontStyle, TextMeasurer, ThemePalette,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct GdiStats {
    pub objects_created: u32,
    pub objects_deleted: u32,
    pub dcs_created: u32,
    pub dcs_deleted: u32,
    pub paint_count: u32,
    pub backbuffer_reallocs: u32,
}

impl GdiStats {
    pub fn live_objects(&self) -> i32 {
        self.objects_created as i32 - self.objects_deleted as i32
    }

    pub fn live_dcs(&self) -> i32 {
        self.dcs_created as i32 - self.dcs_deleted as i32
    }
}

/// Pre-created & cached `HFONT`, `HBRUSH`, `HPEN` handles plus a reusable measurement `HDC`.
pub struct GdiCache {
    pub editor_font: HFONT,
    fonts: Vec<(FontStyle, HFONT)>,
    brushes: Vec<(COLORREF, HBRUSH)>,
    pens: Vec<((COLORREF, i32), HPEN)>,
    pub white_brush: HBRUSH,
    pub splitter_brush: HBRUSH,
    pub dark_splitter_brush: HBRUSH,
    measure_dc: HDC,
    measure_scratch: RefCell<Vec<u8>>,
    pub stats: GdiStats,
}

impl GdiCache {
    pub fn new() -> Self {
        let mut stats = GdiStats::default();
        let screen_dc = unsafe { GetDC(ptr::null_mut()) };
        let measure_dc = unsafe { CreateCompatibleDC(screen_dc) };
        if !screen_dc.is_null() {
            unsafe { ReleaseDC(ptr::null_mut(), screen_dc) };
        }
        stats.dcs_created += 1;

        let mut cache = Self {
            editor_font: ptr::null_mut(),
            fonts: Vec::with_capacity(32),
            brushes: Vec::with_capacity(24),
            pens: Vec::with_capacity(24),
            white_brush: ptr::null_mut(),
            splitter_brush: ptr::null_mut(),
            dark_splitter_brush: ptr::null_mut(),
            measure_dc,
            measure_scratch: RefCell::new(Vec::with_capacity(256)),
            stats,
        };

        cache.editor_font = cache.create_font_raw("Courier New", -14, FW_NORMAL, false, false, false, true);
        cache.white_brush = cache.create_brush(rgb(0xFF, 0xFF, 0xFF));
        cache.splitter_brush = cache.create_brush(rgb(0xC0, 0xC0, 0xC0));
        cache.dark_splitter_brush = cache.create_brush(rgb(0x3C, 0x3C, 0x3C));

        // Pre-populate core brushes & pens for BOTH Light and Dark themes so toggling
        // dark mode or painting allocates ZERO GDI objects.
        for theme in [ThemePalette::LIGHT, ThemePalette::DARK] {
            for &c in &[
                theme.bg,
                theme.code_bg,
                theme.inline_code_bg,
                theme.quote_bar,
                theme.quote_bg,
                theme.table_header_bg,
                theme.table_stripe_bg,
            ] {
                let _ = cache.get_or_create_brush(c);
            }
            for &(c, w) in &[
                (theme.code_border, 1),
                (theme.rule, 1),
                (theme.rule, 2),
                (theme.table_border, 1),
            ] {
                let _ = cache.get_or_create_pen(c, w);
            }
        }

        // Pre-populate standard fonts (body, headings H1-H6, bold/italic/strike/link/mono)
        let presets = [
            FontStyle::default(),
            FontStyle { bold: true, ..FontStyle::default() },
            FontStyle { italic: true, ..FontStyle::default() },
            FontStyle { bold: true, italic: true, ..FontStyle::default() },
            FontStyle { strikethrough: true, ..FontStyle::default() },
            FontStyle { link: true, ..FontStyle::default() },
            FontStyle { monospace: true, ..FontStyle::default() },
            FontStyle { monospace: true, bold: true, ..FontStyle::default() },
            FontStyle { heading: 1, bold: true, ..FontStyle::default() },
            FontStyle { heading: 2, bold: true, ..FontStyle::default() },
            FontStyle { heading: 3, bold: true, ..FontStyle::default() },
            FontStyle { heading: 4, bold: true, ..FontStyle::default() },
            FontStyle { heading: 5, bold: true, ..FontStyle::default() },
            FontStyle { heading: 6, bold: true, ..FontStyle::default() },
        ];
        for s in presets {
            let _ = cache.get_or_create_font(s);
        }

        cache
    }

    fn create_font_raw(
        &mut self,
        face: &str,
        height: i32,
        weight: i32,
        italic: bool,
        underline: bool,
        strikeout: bool,
        fixed: bool,
    ) -> HFONT {
        let mut lf: LOGFONTA = unsafe { core::mem::zeroed() };
        lf.lfHeight = height;
        lf.lfWeight = weight;
        lf.lfItalic = if italic { 1 } else { 0 };
        lf.lfUnderline = if underline { 1 } else { 0 };
        lf.lfStrikeOut = if strikeout { 1 } else { 0 };
        lf.lfCharSet = ANSI_CHARSET as u8;
        lf.lfOutPrecision = OUT_DEFAULT_PRECIS as u8;
        lf.lfClipPrecision = CLIP_DEFAULT_PRECIS as u8;
        lf.lfQuality = DEFAULT_QUALITY as u8;
        lf.lfPitchAndFamily = if fixed {
            (FIXED_PITCH | FF_MODERN) as u8
        } else {
            (DEFAULT_PITCH | FF_SWISS) as u8
        };
        let bytes = face.as_bytes();
        let n = bytes.len().min(31);
        lf.lfFaceName[..n].copy_from_slice(&bytes[..n]);
        let h = unsafe { CreateFontIndirectA(&lf) };
        if !h.is_null() {
            self.stats.objects_created += 1;
        }
        h
    }

    fn create_brush(&mut self, color: COLORREF) -> HBRUSH {
        let h = unsafe { CreateSolidBrush(color) };
        if !h.is_null() {
            self.stats.objects_created += 1;
        }
        h
    }

    pub fn get_or_create_font(&mut self, style: FontStyle) -> HFONT {
        for &(s, h) in &self.fonts {
            if s == style {
                return h;
            }
        }
        let height = match style.heading {
            1 => -24,
            2 => -20,
            3 => -17,
            4 => -15,
            5 => -14,
            6 => -13,
            _ if style.monospace => -13,
            _ => -14,
        };
        let weight = if style.bold || style.heading > 0 {
            FW_BOLD
        } else {
            FW_NORMAL
        };
        let face = if style.monospace { "Courier New" } else { "Arial" };
        let h = self.create_font_raw(
            face,
            height,
            weight,
            style.italic,
            style.link,
            style.strikethrough,
            style.monospace,
        );
        self.fonts.push((style, h));
        h
    }

    pub fn lookup_font(&self, style: FontStyle) -> HFONT {
        for &(s, h) in &self.fonts {
            if s == style {
                return h;
            }
        }
        self.fonts.first().map(|&(_, h)| h).unwrap_or(ptr::null_mut())
    }

    pub fn get_or_create_brush(&mut self, color: COLORREF) -> HBRUSH {
        for &(c, h) in &self.brushes {
            if c == color {
                return h;
            }
        }
        let h = self.create_brush(color);
        self.brushes.push((color, h));
        h
    }

    pub fn lookup_brush(&self, color: COLORREF) -> HBRUSH {
        for &(c, h) in &self.brushes {
            if c == color {
                return h;
            }
        }
        self.white_brush
    }

    pub fn get_or_create_pen(&mut self, color: COLORREF, width: i32) -> HPEN {
        let key = (color, width.max(1));
        for &(k, h) in &self.pens {
            if k == key {
                return h;
            }
        }
        let h = unsafe { CreatePen(PS_SOLID, key.1, key.0) };
        if !h.is_null() {
            self.stats.objects_created += 1;
        }
        self.pens.push((key, h));
        h
    }

    pub fn lookup_pen(&self, color: COLORREF, width: i32) -> HPEN {
        let key = (color, width.max(1));
        for &(k, h) in &self.pens {
            if k == key {
                return h;
            }
        }
        self.pens.first().map(|&(_, h)| h).unwrap_or(ptr::null_mut())
    }

    /// Ensure every font/brush/pen referenced by a laid-out document is cached BEFORE `WM_PAINT`.
    pub fn precache_for_layout(&mut self, layout: &DocumentLayout) {
        for op in &layout.ops {
            match *op {
                DrawOp::FillRect { color, .. } => {
                    let _ = self.get_or_create_brush(color);
                }
                DrawOp::StrokeRect { color, width, .. } | DrawOp::Line { color, width, .. } => {
                    let _ = self.get_or_create_pen(color, width);
                }
                DrawOp::TextRun { style, .. } => {
                    let _ = self.get_or_create_font(style);
                }
                DrawOp::Checkbox { .. } => {}
            }
        }
    }

    pub fn measure_text(&self, text: &str, style: FontStyle) -> (i32, i32) {
        let hfont = self.lookup_font(style);
        let mut scratch = self.measure_scratch.borrow_mut();
        scratch.clear();
        utf8_to_cp1252_into(text, &mut scratch);

        let mut sz = SIZE { cx: 0, cy: 14 };
        if !self.measure_dc.is_null() && !hfont.is_null() {
            unsafe {
                let old = SelectObject(self.measure_dc, hfont);
                let mut fit: i32 = 0;
                if scratch.is_empty() {
                    GetTextExtentExPointA(
                        self.measure_dc,
                        b"Ag".as_ptr(),
                        2,
                        0,
                        &mut fit,
                        ptr::null_mut(),
                        &mut sz,
                    );
                    sz.cx = 0;
                } else {
                    GetTextExtentExPointA(
                        self.measure_dc,
                        scratch.as_ptr(),
                        scratch.len() as i32,
                        32767,
                        &mut fit,
                        ptr::null_mut(),
                        &mut sz,
                    );
                }
                SelectObject(self.measure_dc, old);
            }
        }
        (sz.cx.max(0), sz.cy.max(12))
    }
}

impl Drop for GdiCache {
    fn drop(&mut self) {
        unsafe {
            if !self.editor_font.is_null() {
                DeleteObject(self.editor_font);
                self.stats.objects_deleted += 1;
            }
            if !self.white_brush.is_null() {
                DeleteObject(self.white_brush);
                self.stats.objects_deleted += 1;
            }
            if !self.splitter_brush.is_null() {
                DeleteObject(self.splitter_brush);
                self.stats.objects_deleted += 1;
            }
            if !self.dark_splitter_brush.is_null() {
                DeleteObject(self.dark_splitter_brush);
                self.stats.objects_deleted += 1;
            }
            for &(_, h) in &self.fonts {
                if !h.is_null() {
                    DeleteObject(h);
                    self.stats.objects_deleted += 1;
                }
            }
            for &(_, h) in &self.brushes {
                if !h.is_null() {
                    DeleteObject(h);
                    self.stats.objects_deleted += 1;
                }
            }
            for &(_, h) in &self.pens {
                if !h.is_null() {
                    DeleteObject(h);
                    self.stats.objects_deleted += 1;
                }
            }
            if !self.measure_dc.is_null() {
                DeleteDC(self.measure_dc);
                self.stats.dcs_deleted += 1;
            }
        }
    }
}

/// Wrapper implementing `md98_core::TextMeasurer` using `GdiCache::measure_text` (`GetTextExtentExPointA`).
pub struct Win32Measurer<'a> {
    pub cache: &'a GdiCache,
}

impl<'a> TextMeasurer for Win32Measurer<'a> {
    fn measure(&self, text: &str, style: FontStyle) -> (i32, i32) {
        self.cache.measure_text(text, style)
    }
}

/// Viewport-only offscreen backbuffer (`CreateCompatibleBitmap` sized strictly to visible preview pane).
pub struct ViewportBackBuffer {
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    width: i32,
    height: i32,
}

impl ViewportBackBuffer {
    pub fn new() -> Self {
        Self {
            mem_dc: ptr::null_mut(),
            bitmap: ptr::null_mut(),
            old_bitmap: ptr::null_mut(),
            width: 0,
            height: 0,
        }
    }

    pub fn ensure(&mut self, hdc: HDC, w: i32, h: i32, stats: &mut GdiStats) -> HDC {
        let w = w.max(1);
        let h = h.max(1);
        if self.mem_dc.is_null() {
            self.mem_dc = unsafe { CreateCompatibleDC(hdc) };
            stats.dcs_created += 1;
        }
        if self.bitmap.is_null() || self.width != w || self.height != h {
            self.release_bitmap(stats);
            let bmp = unsafe { CreateCompatibleBitmap(hdc, w, h) };
            if !bmp.is_null() {
                stats.objects_created += 1;
                stats.backbuffer_reallocs += 1;
                self.old_bitmap = unsafe { SelectObject(self.mem_dc, bmp) };
                self.bitmap = bmp;
                self.width = w;
                self.height = h;
            }
        }
        self.mem_dc
    }

    fn release_bitmap(&mut self, stats: &mut GdiStats) {
        if !self.bitmap.is_null() {
            unsafe {
                if !self.old_bitmap.is_null() {
                    SelectObject(self.mem_dc, self.old_bitmap);
                    self.old_bitmap = ptr::null_mut();
                }
                DeleteObject(self.bitmap);
            }
            stats.objects_deleted += 1;
            self.bitmap = ptr::null_mut();
            self.width = 0;
            self.height = 0;
        }
    }

    pub fn destroy(&mut self, stats: &mut GdiStats) {
        self.release_bitmap(stats);
        if !self.mem_dc.is_null() {
            unsafe { DeleteDC(self.mem_dc) };
            stats.dcs_deleted += 1;
            self.mem_dc = ptr::null_mut();
        }
    }
}

/// Paint visible `DrawOp`s into the viewport-sized backbuffer and `BitBlt` to screen.
pub fn paint_preview(
    hdc: HDC,
    view_w: i32,
    view_h: i32,
    scroll_x: i32,
    scroll_y: i32,
    bg_color: COLORREF,
    layout: &DocumentLayout,
    cache: &mut GdiCache,
    backbuf: &mut ViewportBackBuffer,
) {
    if view_w <= 0 || view_h <= 0 {
        return;
    }
    let mem_dc = backbuf.ensure(hdc, view_w, view_h, &mut cache.stats);
    if mem_dc.is_null() {
        return;
    }
    cache.stats.paint_count += 1;

    unsafe {
        let full = RECT {
            left: 0,
            top: 0,
            right: view_w,
            bottom: view_h,
        };
        FillRect(mem_dc, &full, cache.lookup_brush(bg_color));
        SetBkMode(mem_dc, TRANSPARENT);

        let vis_top = scroll_y - 32;
        let vis_bot = scroll_y + view_h + 32;
        let null_brush = GetStockObject(NULL_BRUSH);

        for op in &layout.ops {
            let (y0, y1) = op.y_bounds();
            if y1 < vis_top || y0 > vis_bot {
                continue;
            }
            match *op {
                DrawOp::FillRect { x, y, w, h, color } => {
                    let rc = RECT {
                        left: x - scroll_x,
                        top: y - scroll_y,
                        right: x + w - scroll_x,
                        bottom: y + h - scroll_y,
                    };
                    FillRect(mem_dc, &rc, cache.lookup_brush(color));
                }
                DrawOp::StrokeRect { x, y, w, h, color, width } => {
                    let pen = cache.lookup_pen(color, width);
                    let old_p = SelectObject(mem_dc, pen);
                    let old_b = SelectObject(mem_dc, null_brush);
                    Rectangle(
                        mem_dc,
                        x - scroll_x,
                        y - scroll_y,
                        x + w - scroll_x,
                        y + h - scroll_y,
                    );
                    SelectObject(mem_dc, old_b);
                    SelectObject(mem_dc, old_p);
                }
                DrawOp::Line { x1, y1, x2, y2, color, width } => {
                    let pen = cache.lookup_pen(color, width);
                    let old_p = SelectObject(mem_dc, pen);
                    MoveToEx(mem_dc, x1 - scroll_x, y1 - scroll_y, ptr::null_mut());
                    LineTo(mem_dc, x2 - scroll_x, y2 - scroll_y);
                    SelectObject(mem_dc, old_p);
                }
                DrawOp::TextRun {
                    x,
                    y,
                    cp1252_offset,
                    cp1252_len,
                    style,
                    color,
                    ..
                } => {
                    if cp1252_len == 0 {
                        continue;
                    }
                    let bytes = layout.text_cp1252(cp1252_offset, cp1252_len);
                    let font = cache.lookup_font(style);
                    let old_f = SelectObject(mem_dc, font);
                    SetTextColor(mem_dc, color);
                    TextOutA(mem_dc, x - scroll_x, y - scroll_y, bytes.as_ptr(), bytes.len() as i32);
                    SelectObject(mem_dc, old_f);
                }
                DrawOp::Checkbox { x, y, size, checked } => {
                    let mut rc = RECT {
                        left: x - scroll_x,
                        top: y - scroll_y,
                        right: x + size - scroll_x,
                        bottom: y + size - scroll_y,
                    };
                    let state = DFCS_BUTTONCHECK | if checked { DFCS_CHECKED } else { 0 };
                    DrawFrameControl(mem_dc, &mut rc, DFC_BUTTON, state);
                }
            }
        }

        BitBlt(hdc, 0, 0, view_w, view_h, mem_dc, 0, 0, SRCCOPY);
    }
}
