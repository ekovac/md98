//! Native Win32 SDI shell, RichEdit 2.0 editor, draggable splitter, file I/O,
//! debounced live preview, proportional scroll sync, Find/Replace, Reflow, and CLI flags.

#![cfg(windows)]

use crate::gdi::{paint_preview, GdiCache, ViewportBackBuffer, Win32Measurer};
use crate::prefs::Prefs;
use crate::types::{
    ControlId, FindReplaceAction, MenuId, MessageIcon, SavePromptChoice, ScrollAction, ScrollBar,
    TimerId, ViewMode, WrapWidth,
};
use crate::win32::{self, RichEdit};
use crate::win32_ffi::*;
use core::ptr;
use md98_core::{
    compute_live_wrap_break, cp1252_to_utf8, find_cp1252, is_line_in_fenced_code,
    layout_markdown_with_theme, normalize_to_lf, reflow_markdown, replace_all_text, utf8_to_cp1252,
    DocumentLayout, LayoutConfig, ThemePalette,
};

const DEBOUNCE_MS: u32 = 250;
const SPLITTER_WIDTH: i32 = 4;

#[derive(Default)]
struct CliOptions {
    fixture_path: Option<String>,
    save_test_path: Option<String>,
    headless_exit_ms: Option<u32>,
    stress_edits: u32,
    dump_metrics: bool,
    dark_mode: bool,
    wrap_width: Option<WrapWidth>,
    live_wrap: bool,
    reflow_on_save: bool,
}

fn parse_cli() -> CliOptions {
    let mut opts = CliOptions::default();
    let cmd = win32::command_line_utf8();
    if cmd.is_empty() {
        return opts;
    }

    // Tokenize honoring double quotes
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in cmd.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !cur.is_empty() {
                    tokens.push(core::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }

    let mut i = 1usize; // skip argv[0]
    while i < tokens.len() {
        match tokens[i].as_str() {
            "--test-fixture" if i + 1 < tokens.len() => {
                opts.fixture_path = Some(tokens[i + 1].clone());
                i += 2;
            }
            "--save-test" if i + 1 < tokens.len() => {
                opts.save_test_path = Some(tokens[i + 1].clone());
                i += 2;
            }
            "--headless-exit" if i + 1 < tokens.len() => {
                opts.headless_exit_ms = tokens[i + 1].parse::<u32>().ok();
                i += 2;
            }
            "--stress-edits" if i + 1 < tokens.len() => {
                opts.stress_edits = tokens[i + 1].parse::<u32>().unwrap_or(0);
                i += 2;
            }
            "--dump-metrics" => {
                opts.dump_metrics = true;
                i += 1;
            }
            "--dark-mode" => {
                opts.dark_mode = true;
                i += 1;
            }
            "--wrap-width" if i + 1 < tokens.len() => {
                opts.wrap_width = tokens[i + 1].parse::<u32>().ok().and_then(WrapWidth::from_u32);
                i += 2;
            }
            "--live-wrap" => {
                opts.live_wrap = true;
                i += 1;
            }
            "--reflow-on-save" => {
                opts.reflow_on_save = true;
                i += 1;
            }
            arg if !arg.starts_with('-') && opts.fixture_path.is_none() => {
                opts.fixture_path = Some(arg.to_string());
                i += 1;
            }
            _ => i += 1,
        }
    }
    opts
}

struct AppState {
    hwnd_main: HWND,
    editor: RichEdit,
    hwnd_preview: HWND,
    hwnd_status: HWND,
    hmenu: HMENU,
    hcursor_sizewe: HCURSOR,
    hriched_dll: HMODULE,

    // Native COMDLG32 modeless Find / Replace dialog state (pinned inside Box<AppState>)
    msg_find_replace: UINT,
    hwnd_find: HWND,
    find_is_replace: bool,
    find_fr: FINDREPLACEA,
    find_buf: [u8; 256],
    replace_buf: [u8; 256],
    find_flags: DWORD,

    // User preferences (persisted in HKCU\Software\md98)
    prefs: Prefs,

    gdi: GdiCache,
    backbuf: ViewportBackBuffer,
    layout: DocumentLayout,

    // Reusable scratch buffers to minimize heap churn on 40 MB RAM
    raw_ansi_buf: Vec<u8>,
    utf8_buf: String,

    current_file: Option<String>,
    modified: bool,
    suppress_change: bool,
    headless_closing: bool,

    dragging_splitter: bool,
    splitter_rect: RECT,

    // Preserved editor cursor & scroll across F11 view-mode toggles
    saved_edit_sel: CHARRANGE,
    saved_edit_first_line: i32,
    has_saved_edit_pos: bool,

    scroll_x: i32,
    scroll_y: i32,
    preview_w: i32,
    preview_h: i32,
    last_layout_ms: u32,

    cli: CliOptions,
    gdi_live_initial: i32,
    stress_completed: u32,
    save_test_bytes: usize,
    replace_count: usize,
}

impl AppState {
    fn current_theme(&self) -> ThemePalette {
        if self.prefs.dark_mode {
            ThemePalette::DARK
        } else {
            ThemePalette::LIGHT
        }
    }

    fn sync_all_menu_checks(&self) {
        let p = &self.prefs;
        win32::check_menu_item(self.hmenu, MenuId::ViewSplit, p.view_mode == ViewMode::Split);
        win32::check_menu_item(self.hmenu, MenuId::ViewEditor, p.view_mode == ViewMode::EditorOnly);
        win32::check_menu_item(self.hmenu, MenuId::ViewPreview, p.view_mode == ViewMode::PreviewOnly);
        win32::check_menu_item(self.hmenu, MenuId::ViewSyncScroll, p.sync_scroll);
        win32::check_menu_item(self.hmenu, MenuId::ViewDarkMode, p.dark_mode);
        win32::check_menu_item(self.hmenu, MenuId::EditWrap72, p.wrap_width == WrapWidth::Cols72);
        win32::check_menu_item(self.hmenu, MenuId::EditWrap80, p.wrap_width == WrapWidth::Cols80);
        win32::check_menu_item(self.hmenu, MenuId::EditWrap100, p.wrap_width == WrapWidth::Cols100);
        win32::check_menu_item(self.hmenu, MenuId::EditLiveWrap, p.live_wrap);
        win32::check_menu_item(self.hmenu, MenuId::EditReflowOnSave, p.reflow_on_save);
    }

    fn save_editor_cursor_and_scroll(&mut self) {
        self.saved_edit_sel = self.editor.get_selection();
        self.saved_edit_first_line = self.editor.first_visible_line();
        self.has_saved_edit_pos = true;
    }

    fn restore_editor_cursor_and_scroll(&self) {
        if !self.has_saved_edit_pos {
            return;
        }
        self.editor.set_selection(self.saved_edit_sel);
        self.editor.scroll_to_line(self.saved_edit_first_line);
    }

    fn apply_editor_theme(&mut self) {
        let theme = self.current_theme();
        let prev = self.suppress_change;
        self.suppress_change = true;
        self.editor.apply_theme(theme.bg, theme.text);
        self.suppress_change = prev;
    }

    fn toggle_dark_mode(&mut self) {
        self.prefs.dark_mode = !self.prefs.dark_mode;
        win32::check_menu_item(self.hmenu, MenuId::ViewDarkMode, self.prefs.dark_mode);
        self.prefs.save();
        self.apply_editor_theme();
        self.relayout_preview_from_cached_utf8();
        win32::invalidate(self.hwnd_main, true);
    }

    fn set_view_mode(&mut self, new_mode: ViewMode) {
        if new_mode == self.prefs.view_mode {
            return;
        }
        let prev_mode = self.prefs.view_mode;

        if prev_mode != ViewMode::PreviewOnly {
            self.save_editor_cursor_and_scroll();
            win32::kill_timer(self.hwnd_main, TimerId::Debounce);
            self.read_editor_to_utf8();
        }

        self.prefs.view_mode = new_mode;
        self.sync_all_menu_checks();
        self.layout_children();
        self.prefs.save();

        match new_mode {
            ViewMode::EditorOnly => {
                self.restore_editor_cursor_and_scroll();
                win32::set_focus(self.editor.0);
                self.update_status_bar();
            }
            ViewMode::PreviewOnly => {
                if prev_mode == ViewMode::EditorOnly {
                    self.relayout_preview_from_cached_utf8();
                }
                win32::set_focus(self.hwnd_preview);
            }
            ViewMode::Split => {
                if prev_mode == ViewMode::PreviewOnly {
                    self.restore_editor_cursor_and_scroll();
                }
                self.relayout_preview_from_cached_utf8();
                win32::set_focus(self.editor.0);
            }
        }
    }

    fn toggle_preview_editor_f11(&mut self) {
        if self.prefs.view_mode == ViewMode::PreviewOnly {
            self.set_view_mode(ViewMode::EditorOnly);
        } else {
            self.set_view_mode(ViewMode::PreviewOnly);
        }
    }

    fn set_wrap_width(&mut self, width: WrapWidth) {
        self.prefs.wrap_width = width;
        self.sync_all_menu_checks();
        self.prefs.save();
        self.update_status_bar();
    }

    fn toggle_live_wrap(&mut self) {
        self.prefs.live_wrap = !self.prefs.live_wrap;
        win32::check_menu_item(self.hmenu, MenuId::EditLiveWrap, self.prefs.live_wrap);
        self.prefs.save();
        self.update_status_bar();
    }

    fn toggle_reflow_on_save(&mut self) {
        self.prefs.reflow_on_save = !self.prefs.reflow_on_save;
        win32::check_menu_item(
            self.hmenu,
            MenuId::EditReflowOnSave,
            self.prefs.reflow_on_save,
        );
        self.prefs.save();
        self.update_status_bar();
    }

    fn toggle_sync_scroll(&mut self) {
        self.prefs.sync_scroll = !self.prefs.sync_scroll;
        win32::check_menu_item(self.hmenu, MenuId::ViewSyncScroll, self.prefs.sync_scroll);
        self.prefs.save();
    }

    /// Reflow the entire document in-place while preserving caret & scroll position.
    fn reflow_full_document(&mut self) -> bool {
        let cr = self.editor.get_selection();
        let first_line = self.editor.first_visible_line();
        self.read_editor_to_utf8();
        let reflowed = reflow_markdown(&self.utf8_buf, self.prefs.wrap_width.columns());
        if reflowed == self.utf8_buf {
            return false;
        }
        let max_cp = reflowed.chars().count() as i32;
        self.suppress_change = true;
        self.editor.set_text_lf(&reflowed);
        self.editor.set_modified(true);
        self.apply_editor_theme();
        let pos = cr.cpMin.clamp(0, max_cp);
        self.editor.set_selection(CHARRANGE { cpMin: pos, cpMax: pos });
        self.editor.scroll_to_line(first_line);
        self.suppress_change = false;
        self.utf8_buf = reflowed;
        self.relayout_preview_from_cached_utf8();
        true
    }

    /// On-demand reflow: reflows current selection if non-empty, otherwise the entire document.
    fn reflow_editor_text(&mut self) {
        let cr = self.editor.get_selection();
        if cr.cpMax > cr.cpMin {
            let sel_bytes = self.editor.get_selected_cp1252(cr);
            let sel_utf8 = normalize_to_lf(&cp1252_to_utf8(&sel_bytes));
            let reflowed = reflow_markdown(&sel_utf8, self.prefs.wrap_width.columns());
            if reflowed != sel_utf8 {
                self.suppress_change = true;
                self.editor.replace_selection_lf(&reflowed, true);
                self.editor.set_modified(true);
                self.apply_editor_theme();
                self.suppress_change = false;
                self.modified = true;
                self.update_title();
                self.relayout_preview_from_editor();
            }
            return;
        }

        if self.reflow_full_document() {
            self.modified = true;
            self.update_title();
        }
    }

    /// Live auto-wrap: breaks the current hard paragraph at a word boundary when crossing `wrap_width`.
    fn maybe_live_wrap_current_line(&mut self) {
        if !self.prefs.live_wrap || self.suppress_change {
            return;
        }
        let cr = self.editor.get_selection();
        if cr.cpMin != cr.cpMax {
            return;
        }
        self.read_editor_to_utf8();
        // Locate hard-newline boundaries directly in CP1252-LF offsets (not soft-wrapped display lines)
        let cp1252 = utf8_to_cp1252(&self.utf8_buf);
        let caret = (cr.cpMin.max(0) as usize).min(cp1252.len());
        let hard_start = cp1252[..caret]
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let hard_end = cp1252[caret..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| caret + i)
            .unwrap_or(cp1252.len());
        let hard_line_idx = cp1252[..hard_start].iter().filter(|&&b| b == b'\n').count();
        let line_str = cp1252_to_utf8(&cp1252[hard_start..hard_end]);

        let in_fence = is_line_in_fenced_code(&self.utf8_buf, hard_line_idx);
        let Some((break_char_off, insert_str)) =
            compute_live_wrap_break(&line_str, self.prefs.wrap_width.columns(), in_fence)
        else {
            return;
        };

        let break_cp = (hard_start + break_char_off) as i32;
        if cr.cpMin <= break_cp {
            return;
        }

        let cont_chars = insert_str.chars().count().saturating_sub(2) as i32;
        let new_pos = cr.cpMin + cont_chars;
        self.suppress_change = true;
        self.editor.set_selection(CHARRANGE {
            cpMin: break_cp,
            cpMax: break_cp + 1,
        });
        self.editor.replace_selection_raw_cp1252(&insert_str, true);
        self.editor.set_selection(CHARRANGE {
            cpMin: new_pos,
            cpMax: new_pos,
        });
        self.editor.scroll_caret();
        self.suppress_change = false;
    }

    fn find_what_str(&self) -> String {
        let len = self.find_buf.iter().position(|&b| b == 0).unwrap_or(self.find_buf.len());
        cp1252_to_utf8(&self.find_buf[..len])
    }

    fn replace_with_str(&self) -> String {
        let len = self.replace_buf.iter().position(|&b| b == 0).unwrap_or(self.replace_buf.len());
        cp1252_to_utf8(&self.replace_buf[..len])
    }

    fn find_dialog_owner(&self) -> HWND {
        if !self.hwnd_find.is_null() {
            self.hwnd_find
        } else {
            self.hwnd_main
        }
    }

    fn open_find_dialog(&mut self, replace: bool) {
        if self.prefs.view_mode == ViewMode::PreviewOnly {
            self.set_view_mode(ViewMode::EditorOnly);
        }
        if !self.hwnd_find.is_null() {
            if self.find_is_replace == replace {
                win32::set_focus(self.hwnd_find);
                return;
            }
            unsafe {
                DestroyWindow(self.hwnd_find);
            }
            self.hwnd_find = ptr::null_mut();
        }

        // Pre-fill Find box with current single-line selection if non-empty
        let cr = self.editor.get_selection();
        if cr.cpMax > cr.cpMin && (cr.cpMax - cr.cpMin) < 128 {
            let sel = self.editor.get_selected_cp1252(cr);
            if !sel.contains(&b'\r') && !sel.contains(&b'\n') && !sel.is_empty() {
                let n = sel.len().min(self.find_buf.len() - 1);
                self.find_buf[..n].copy_from_slice(&sel[..n]);
                self.find_buf[n] = 0;
            }
        }

        self.find_fr = unsafe { core::mem::zeroed() };
        self.find_fr.lStructSize = core::mem::size_of::<FINDREPLACEA>() as DWORD;
        self.find_fr.hwndOwner = self.hwnd_main;
        self.find_fr.Flags = self.find_flags;
        self.find_fr.lpstrFindWhat = self.find_buf.as_mut_ptr();
        self.find_fr.wFindWhatLen = self.find_buf.len() as WORD;
        self.find_fr.lpstrReplaceWith = self.replace_buf.as_mut_ptr();
        self.find_fr.wReplaceWithLen = self.replace_buf.len() as WORD;

        self.find_is_replace = replace;
        self.hwnd_find = unsafe {
            if replace {
                ReplaceTextA(&mut self.find_fr)
            } else {
                FindTextA(&mut self.find_fr)
            }
        };
    }

    fn do_find_next(&mut self, notify_not_found: bool) -> bool {
        let needle_utf8 = self.find_what_str();
        if needle_utf8.is_empty() {
            self.open_find_dialog(false);
            return false;
        }
        self.read_editor_to_utf8();
        let hay = utf8_to_cp1252(&self.utf8_buf);
        let ndl = utf8_to_cp1252(&needle_utf8);

        let cr = self.editor.get_selection();
        let down = (self.find_flags & FR_DOWN) != 0;
        let match_case = (self.find_flags & FR_MATCHCASE) != 0;
        let whole_word = (self.find_flags & FR_WHOLEWORD) != 0;
        let start_pos = if down {
            cr.cpMax.max(0) as usize
        } else {
            cr.cpMin.max(0) as usize
        };

        if let Some((m_start, m_end)) = find_cp1252(&hay, &ndl, start_pos, down, match_case, whole_word) {
            self.editor.set_selection(CHARRANGE {
                cpMin: m_start as i32,
                cpMax: m_end as i32,
            });
            self.editor.scroll_caret();
            self.update_status_cursor_pos();
            true
        } else {
            if notify_not_found {
                win32::message_box_ok(
                    self.find_dialog_owner(),
                    "md98",
                    &format!("Cannot find \"{}\".", needle_utf8),
                    MessageIcon::Info,
                );
            }
            false
        }
    }

    fn do_replace_current(&mut self) {
        let needle_utf8 = self.find_what_str();
        if needle_utf8.is_empty() {
            return;
        }
        let cr = self.editor.get_selection();
        let match_case = (self.find_flags & FR_MATCHCASE) != 0;
        let whole_word = (self.find_flags & FR_WHOLEWORD) != 0;

        let mut selection_matches = false;
        if cr.cpMax > cr.cpMin {
            self.read_editor_to_utf8();
            let hay = utf8_to_cp1252(&self.utf8_buf);
            let ndl = utf8_to_cp1252(&needle_utf8);
            let s = cr.cpMin.max(0) as usize;
            let e = cr.cpMax.max(0) as usize;
            if e - s == ndl.len() && e <= hay.len() {
                if let Some((ms, me)) = find_cp1252(&hay, &ndl, s, true, match_case, whole_word) {
                    selection_matches = ms == s && me == e;
                }
            }
        }

        if selection_matches {
            let repl_utf8 = self.replace_with_str();
            self.suppress_change = true;
            self.editor.replace_selection_lf(&repl_utf8, true);
            self.editor.set_modified(true);
            self.apply_editor_theme();
            self.suppress_change = false;
            self.modified = true;
            self.replace_count += 1;
            self.update_title();
            self.relayout_preview_from_editor();
        }
        let _ = self.do_find_next(true);
    }

    fn do_replace_all(&mut self) {
        let needle_utf8 = self.find_what_str();
        if needle_utf8.is_empty() {
            return;
        }
        let repl_utf8 = self.replace_with_str();
        let match_case = (self.find_flags & FR_MATCHCASE) != 0;
        let whole_word = (self.find_flags & FR_WHOLEWORD) != 0;

        let first_line = self.editor.first_visible_line();
        self.read_editor_to_utf8();
        let (new_utf8, count) =
            replace_all_text(&self.utf8_buf, &needle_utf8, &repl_utf8, match_case, whole_word);

        if count > 0 {
            self.suppress_change = true;
            self.editor.set_text_lf(&new_utf8);
            self.editor.set_modified(true);
            self.apply_editor_theme();
            self.editor.scroll_to_line(first_line);
            self.suppress_change = false;
            self.modified = true;
            self.replace_count += count;
            self.update_title();
            self.relayout_preview_from_editor();
        }

        win32::message_box_ok(
            self.find_dialog_owner(),
            "md98",
            &format!("Replaced {} occurrence(s).", count),
            MessageIcon::Info,
        );
    }

    fn handle_find_replace_msg(&mut self) {
        let flags = self.find_fr.Flags;
        self.find_flags = flags & (FR_DOWN | FR_MATCHCASE | FR_WHOLEWORD);

        match FindReplaceAction::from_flags(flags) {
            FindReplaceAction::DialogClosed => {
                self.hwnd_find = ptr::null_mut();
                win32::set_focus(self.editor.0);
            }
            FindReplaceAction::FindNext => {
                let _ = self.do_find_next(true);
            }
            FindReplaceAction::Replace => self.do_replace_current(),
            FindReplaceAction::ReplaceAll => self.do_replace_all(),
            FindReplaceAction::None => {}
        }
    }

    fn update_title(&self) {
        let name = self.current_file.as_deref().unwrap_or("Untitled");
        let star = if self.modified { "*" } else { "" };
        win32::set_window_text(self.hwnd_main, &format!("md98 - {}{}", name, star));
    }

    fn update_status_cursor_pos(&self) {
        let (ln, col) = self.editor.cursor_line_col();
        win32::set_status_part(self.hwnd_status, 1, &format!("Ln {}, Col {}", ln, col));
    }

    fn update_status_bar(&self) {
        self.update_status_cursor_pos();

        let mut wrap_flags = String::new();
        if self.prefs.live_wrap {
            wrap_flags.push('L');
        }
        if self.prefs.reflow_on_save {
            wrap_flags.push('S');
        }
        let wrap_label = format!("Wrap: {}{}", self.prefs.wrap_width.columns(), wrap_flags);

        win32::set_status_part(
            self.hwnd_status,
            0,
            self.current_file.as_deref().unwrap_or("Ready"),
        );
        win32::set_status_part(self.hwnd_status, 2, &format!("{} bytes", self.utf8_buf.len()));
        win32::set_status_part(
            self.hwnd_status,
            3,
            &format!("{} | {} ms", wrap_label, self.last_layout_ms),
        );
        win32::set_status_part(
            self.hwnd_status,
            4,
            &format!("GDI: {}", self.gdi.stats.live_objects()),
        );
    }

    fn layout_children(&mut self) {
        let (total_w, total_h) = win32::client_size(self.hwnd_main);

        let mut status_h = 0;
        if !self.hwnd_status.is_null() {
            unsafe {
                SendMessageA(self.hwnd_status, WM_SIZE, 0, 0);
            }
            status_h = win32::client_size(self.hwnd_status).1;
            let parts = [
                (total_w - 395).max(120),
                (total_w - 300).max(200),
                (total_w - 205).max(280),
                (total_w - 75).max(380),
                -1,
            ];
            win32::set_status_parts(self.hwnd_status, &parts);
        }

        let body_h = (total_h - status_h).max(1);
        if self.prefs.split_pos <= 0 {
            self.prefs.split_pos = (total_w / 2).max(120);
        }
        let max_split = (total_w - SPLITTER_WIDTH - 100).max(100);
        self.prefs.split_pos = self.prefs.split_pos.clamp(100, max_split);

        match self.prefs.view_mode {
            ViewMode::Split => {
                let left_w = self.prefs.split_pos;
                let right_x = left_w + SPLITTER_WIDTH;
                let right_w = (total_w - right_x).max(1);
                self.splitter_rect = RECT {
                    left: left_w,
                    top: 0,
                    right: right_x,
                    bottom: body_h,
                };
                unsafe {
                    MoveWindow(self.editor.0, 0, 0, left_w, body_h, 1);
                    ShowWindow(self.editor.0, SW_SHOW);
                    MoveWindow(self.hwnd_preview, right_x, 0, right_w, body_h, 1);
                    ShowWindow(self.hwnd_preview, SW_SHOW);
                }
            }
            ViewMode::EditorOnly => {
                self.splitter_rect = RECT::default();
                unsafe {
                    MoveWindow(self.editor.0, 0, 0, total_w, body_h, 1);
                    ShowWindow(self.editor.0, SW_SHOW);
                    ShowWindow(self.hwnd_preview, 0);
                }
            }
            ViewMode::PreviewOnly => {
                self.splitter_rect = RECT::default();
                unsafe {
                    ShowWindow(self.editor.0, 0);
                    MoveWindow(self.hwnd_preview, 0, 0, total_w, body_h, 1);
                    ShowWindow(self.hwnd_preview, SW_SHOW);
                }
            }
        }

        if self.prefs.view_mode != ViewMode::EditorOnly {
            let (pw, ph) = win32::client_size(self.hwnd_preview);
            let new_pw = pw.max(1);
            let new_ph = ph.max(1);
            let width_changed = new_pw != self.preview_w;
            let anchor_src = if width_changed && self.scroll_y > 0 {
                Some(self.layout.source_offset_at_y(self.scroll_y))
            } else {
                None
            };
            self.preview_w = new_pw;
            self.preview_h = new_ph;

            if width_changed {
                self.relayout_preview_from_cached_utf8();
                if let Some(src_off) = anchor_src {
                    let max_y = (self.layout.content_height - self.preview_h).max(0);
                    self.scroll_y = self.layout.y_for_source_offset(src_off).clamp(0, max_y);
                    self.update_preview_scrollbars();
                    win32::invalidate(self.hwnd_preview, false);
                }
            } else {
                self.update_preview_scrollbars();
            }
        }
    }

    fn read_editor_to_utf8(&mut self) {
        self.utf8_buf = self.editor.read_to_utf8_lf(&mut self.raw_ansi_buf);
    }

    fn relayout_preview_from_editor(&mut self) {
        self.read_editor_to_utf8();
        self.relayout_preview_from_cached_utf8();
    }

    fn relayout_preview_from_cached_utf8(&mut self) {
        let t0 = win32::tick_count();
        let vp_w = self.preview_w.max(120);
        let theme = self.current_theme();
        {
            let measurer = Win32Measurer { cache: &self.gdi };
            layout_markdown_with_theme(
                &self.utf8_buf,
                vp_w,
                LayoutConfig::default(),
                theme,
                &measurer,
                &mut self.layout,
            );
        }
        self.gdi.precache_for_layout(&self.layout);
        self.last_layout_ms = win32::tick_count().wrapping_sub(t0);
        self.update_preview_scrollbars();
        win32::invalidate(self.hwnd_preview, false);
        self.update_status_bar();
    }

    fn update_preview_scrollbars(&mut self) {
        let max_y = (self.layout.content_height - self.preview_h).max(0);
        self.scroll_y = self.scroll_y.clamp(0, max_y);
        win32::set_scroll_info(
            self.hwnd_preview,
            ScrollBar::Vert,
            self.layout.content_height,
            self.preview_h,
            self.scroll_y,
        );

        let max_x = (self.layout.content_width - self.preview_w).max(0);
        self.scroll_x = self.scroll_x.clamp(0, max_x);
        win32::set_scroll_info(
            self.hwnd_preview,
            ScrollBar::Horz,
            self.layout.content_width,
            self.preview_w,
            self.scroll_x,
        );
    }

    fn sync_preview_scroll_to_editor(&mut self) {
        if !self.prefs.sync_scroll || self.prefs.view_mode != ViewMode::Split {
            return;
        }
        let first_line = self.editor.first_visible_line();
        let line_count = self.editor.line_count();
        let max_scroll = (self.layout.content_height - self.preview_h).max(0);
        if line_count > 1 && max_scroll > 0 {
            let target_y = ((first_line as i64 * max_scroll as i64) / (line_count - 1) as i64) as i32;
            let clamped = target_y.clamp(0, max_scroll);
            if clamped != self.scroll_y {
                self.scroll_y = clamped;
                self.update_preview_scrollbars();
                win32::invalidate(self.hwnd_preview, false);
            }
        }
    }

    fn load_markdown_text(&mut self, utf8: &str, file_path: Option<String>) {
        self.suppress_change = true;
        self.editor.set_text_lf(utf8);
        self.editor.set_modified(false);
        self.apply_editor_theme();
        self.suppress_change = false;
        self.current_file = file_path;
        self.modified = false;
        self.saved_edit_sel = CHARRANGE::default();
        self.saved_edit_first_line = 0;
        self.has_saved_edit_pos = false;
        self.scroll_x = 0;
        self.scroll_y = 0;
        self.update_title();
        self.relayout_preview_from_editor();
    }

    fn open_path(&mut self, path: &str) -> bool {
        if let Some(bytes) = win32::read_file_bytes(path) {
            let text = match core::str::from_utf8(&bytes) {
                Ok(s) => s.to_string(),
                Err(_) => cp1252_to_utf8(&bytes),
            };
            self.load_markdown_text(&text, Some(path.to_string()));
            true
        } else {
            false
        }
    }

    fn save_to_path(&mut self, path: &str) -> bool {
        if self.prefs.reflow_on_save {
            let _ = self.reflow_full_document();
        } else {
            self.read_editor_to_utf8();
        }
        if win32::write_file_bytes(path, self.utf8_buf.as_bytes()) {
            self.current_file = Some(path.to_string());
            self.modified = false;
            self.editor.set_modified(false);
            self.save_test_bytes = self.utf8_buf.len();
            self.update_title();
            self.update_status_bar();
            true
        } else {
            false
        }
    }

    fn open_dialog(&mut self) {
        if let Some(path) = win32::open_file_dialog(self.hwnd_main) {
            if !self.open_path(&path) {
                win32::message_box_ok(
                    self.hwnd_main,
                    "md98",
                    "Failed to open selected file.",
                    MessageIcon::Error,
                );
            }
        }
    }

    fn save_as_dialog(&mut self) -> bool {
        let Some(path) = win32::save_file_dialog(self.hwnd_main, self.current_file.as_deref()) else {
            return false;
        };
        if self.save_to_path(&path) {
            true
        } else {
            win32::message_box_ok(
                self.hwnd_main,
                "md98",
                "Failed to save file.",
                MessageIcon::Error,
            );
            false
        }
    }

    fn save_current_or_prompt(&mut self) -> bool {
        if let Some(path) = self.current_file.clone() {
            if self.save_to_path(&path) {
                true
            } else {
                win32::message_box_ok(
                    self.hwnd_main,
                    "md98",
                    "Failed to save file.",
                    MessageIcon::Error,
                );
                false
            }
        } else {
            self.save_as_dialog()
        }
    }

    /// Prompt the user to save unsaved edits before closing, New, or Open.
    fn confirm_save_if_modified(&mut self) -> bool {
        if !self.modified {
            return true;
        }
        let name = self.current_file.as_deref().unwrap_or("Untitled");
        match win32::confirm_save_changes(self.hwnd_main, name) {
            SavePromptChoice::Save => self.save_current_or_prompt(),
            SavePromptChoice::Discard => true,
            SavePromptChoice::Cancel => false,
        }
    }

    fn handle_menu_command(&mut self, cmd: MenuId) {
        match cmd {
            MenuId::FileNew => {
                if self.confirm_save_if_modified() {
                    self.load_markdown_text("", None);
                }
            }
            MenuId::FileOpen => {
                if self.confirm_save_if_modified() {
                    self.open_dialog();
                }
            }
            MenuId::FileSave => {
                let _ = self.save_current_or_prompt();
            }
            MenuId::FileSaveAs => {
                let _ = self.save_as_dialog();
            }
            MenuId::FileExit => win32::post_close(self.hwnd_main),
            MenuId::EditUndo => self.editor.undo(),
            MenuId::EditCut => self.editor.cut(),
            MenuId::EditCopy => self.editor.copy(),
            MenuId::EditPaste => self.editor.paste(),
            MenuId::EditSelectAll => self.editor.select_all(),
            MenuId::EditFind => self.open_find_dialog(false),
            MenuId::EditFindNext => {
                let _ = self.do_find_next(true);
            }
            MenuId::EditReplace => self.open_find_dialog(true),
            MenuId::EditReflow => self.reflow_editor_text(),
            MenuId::EditLiveWrap => self.toggle_live_wrap(),
            MenuId::EditReflowOnSave => self.toggle_reflow_on_save(),
            MenuId::EditWrap72 => self.set_wrap_width(WrapWidth::Cols72),
            MenuId::EditWrap80 => self.set_wrap_width(WrapWidth::Cols80),
            MenuId::EditWrap100 => self.set_wrap_width(WrapWidth::Cols100),
            MenuId::ViewSplit => self.set_view_mode(ViewMode::Split),
            MenuId::ViewEditor => self.set_view_mode(ViewMode::EditorOnly),
            MenuId::ViewPreview => self.set_view_mode(ViewMode::PreviewOnly),
            MenuId::ViewToggleF11 => self.toggle_preview_editor_f11(),
            MenuId::ViewDarkMode => self.toggle_dark_mode(),
            MenuId::ViewSyncScroll => self.toggle_sync_scroll(),
            MenuId::HelpAbout => win32::message_box_ok(
                self.hwnd_main,
                "About md98",
                "md98 - Native GFM Editor & Live Previewer for Windows 98 SE\r\nWritten in Rust (rust9x) for i586 Pentium+ (40 MB RAM).",
                MessageIcon::Info,
            ),
        }
    }

    fn run_stress_edits(&mut self, count: u32) {
        let base = self.utf8_buf.clone();
        for i in 0..count {
            let extra = format!("\n\nEdit #{}: **bold** *italic* `code_{}`\n", i + 1, i);
            let mut combined = String::with_capacity(base.len() + extra.len());
            combined.push_str(&base);
            combined.push_str(&extra);
            self.suppress_change = true;
            self.editor.set_text_lf(&combined);
            self.suppress_change = false;
            self.relayout_preview_from_editor();
            unsafe {
                UpdateWindow(self.hwnd_preview);
            }
            self.stress_completed += 1;
        }
        self.load_markdown_text(&base, self.current_file.clone());
        unsafe {
            UpdateWindow(self.hwnd_preview);
        }
    }

    fn dump_metrics_file(&self) {
        if !self.cli.dump_metrics {
            return;
        }
        let ms = win32::memory_status();
        let cr = self.editor.get_selection();
        let cur_first_line = self.editor.first_visible_line();
        let p = &self.prefs;
        let report = format!(
            "source_bytes={}\nblocks={}\ndraw_ops={}\ncontent_height={}\nlayout_ms={}\ndark_mode={}\nview_mode={}\nsync_scroll={}\nwrap_width={}\nlive_wrap={}\nreflow_on_save={}\nsplit_pos={}\nreplace_count={}\nedit_sel_min={}\nedit_first_line={}\npreview_scroll_y={}\ngdi_live_initial={}\ngdi_live_final={}\ngdi_delta={}\ngdi_created={}\ngdi_deleted={}\ngdi_live_dcs={}\npaint_count={}\nbackbuffer_reallocs={}\nstress_edits={}\nsave_test_bytes={}\nmem_load={}\ntotal_phys={}\navail_phys={}\n",
            self.utf8_buf.len(),
            self.layout.blocks.len(),
            self.layout.ops.len(),
            self.layout.content_height,
            self.last_layout_ms,
            u8::from(p.dark_mode),
            p.view_mode.as_str(),
            u8::from(p.sync_scroll),
            p.wrap_width.columns(),
            u8::from(p.live_wrap),
            u8::from(p.reflow_on_save),
            p.split_pos,
            self.replace_count,
            cr.cpMin,
            cur_first_line,
            self.scroll_y,
            self.gdi_live_initial,
            self.gdi.stats.live_objects(),
            self.gdi.stats.live_objects() - self.gdi_live_initial,
            self.gdi.stats.objects_created,
            self.gdi.stats.objects_deleted,
            self.gdi.stats.live_dcs(),
            self.gdi.stats.paint_count,
            self.gdi.stats.backbuffer_reallocs,
            self.stress_completed,
            self.save_test_bytes,
            ms.dwMemoryLoad,
            ms.dwTotalPhys,
            ms.dwAvailPhys,
        );
        let _ = win32::write_file_bytes("C:\\MD98LOG.TXT", report.as_bytes());
    }
}

unsafe fn get_state(hwnd: HWND) -> *mut AppState {
    GetWindowLongA(hwnd, GWL_USERDATA) as *mut AppState
}

unsafe extern "system" fn preview_wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = get_state(hwnd);
    if state_ptr.is_null() {
        return DefWindowProcA(hwnd, msg, wparam, lparam);
    }
    let st = &mut *state_ptr;

    if st.msg_find_replace != 0 && msg == st.msg_find_replace {
        st.handle_find_replace_msg();
        return 0;
    }

    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = core::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint_preview(
                hdc,
                st.preview_w,
                st.preview_h,
                st.scroll_x,
                st.scroll_y,
                st.current_theme().bg,
                &st.layout,
                &mut st.gdi,
                &mut st.backbuf,
            );
            EndPaint(hwnd, &ps);
            0
        }
        WM_VSCROLL => {
            let max_y = (st.layout.content_height - st.preview_h).max(0);
            let page = (st.preview_h - 32).max(24);
            let new_y = match ScrollAction::from_wparam(wparam) {
                ScrollAction::LineUp => st.scroll_y - 24,
                ScrollAction::LineDown => st.scroll_y + 24,
                ScrollAction::PageUp => st.scroll_y - page,
                ScrollAction::PageDown => st.scroll_y + page,
                ScrollAction::Top => 0,
                ScrollAction::Bottom => max_y,
                ScrollAction::ThumbTrack => win32::get_scroll_track_pos(hwnd, ScrollBar::Vert),
                ScrollAction::Other => st.scroll_y,
            };
            st.scroll_y = new_y.clamp(0, max_y);
            st.update_preview_scrollbars();
            win32::invalidate(hwnd, false);
            0
        }
        WM_HSCROLL => {
            let max_x = (st.layout.content_width - st.preview_w).max(0);
            let page = (st.preview_w - 32).max(24);
            let new_x = match ScrollAction::from_wparam(wparam) {
                ScrollAction::LineUp => st.scroll_x - 24,
                ScrollAction::LineDown => st.scroll_x + 24,
                ScrollAction::PageUp => st.scroll_x - page,
                ScrollAction::PageDown => st.scroll_x + page,
                ScrollAction::ThumbTrack => win32::get_scroll_track_pos(hwnd, ScrollBar::Horz),
                _ => st.scroll_x,
            };
            st.scroll_x = new_x.clamp(0, max_x);
            st.update_preview_scrollbars();
            win32::invalidate(hwnd, false);
            0
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(wparam) as i16 as i32;
            let max_y = (st.layout.content_height - st.preview_h).max(0);
            let step = (delta / 120) * 48;
            st.scroll_y = (st.scroll_y - step).clamp(0, max_y);
            st.update_preview_scrollbars();
            win32::invalidate(hwnd, false);
            0
        }
        WM_LBUTTONDOWN => {
            win32::set_focus(hwnd);
            0
        }
        WM_KEYDOWN => {
            let max_y = (st.layout.content_height - st.preview_h).max(0);
            let page = (st.preview_h - 32).max(24);
            let delta = match wparam {
                VK_UP => -24,
                VK_DOWN => 24,
                VK_PRIOR => -page,
                VK_NEXT => page,
                _ => 0,
            };
            if delta != 0 {
                st.scroll_y = (st.scroll_y + delta).clamp(0, max_y);
                st.update_preview_scrollbars();
                win32::invalidate(hwnd, false);
                return 0;
            }
            DefWindowProcA(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn main_wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = get_state(hwnd);
    if state_ptr.is_null() {
        return DefWindowProcA(hwnd, msg, wparam, lparam);
    }
    let st = &mut *state_ptr;

    if st.msg_find_replace != 0 && msg == st.msg_find_replace {
        st.handle_find_replace_msg();
        return 0;
    }

    match msg {
        WM_SIZE => {
            st.layout_children();
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = core::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if st.prefs.view_mode == ViewMode::Split {
                let brush = if st.prefs.dark_mode {
                    st.gdi.dark_splitter_brush
                } else {
                    st.gdi.splitter_brush
                };
                FillRect(hdc, &st.splitter_rect, brush);
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_SETCURSOR => {
            if st.prefs.view_mode == ViewMode::Split
                && (wparam as HWND) == hwnd
                && loword(lparam as usize) == 1
            {
                SetCursor(st.hcursor_sizewe);
                return 1;
            }
            DefWindowProcA(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let x = get_x_lparam(lparam);
            if st.prefs.view_mode == ViewMode::Split
                && x >= st.splitter_rect.left - 2
                && x <= st.splitter_rect.right + 2
            {
                st.dragging_splitter = true;
                SetCapture(hwnd);
                SetCursor(st.hcursor_sizewe);
            }
            0
        }
        WM_MOUSEMOVE => {
            if st.dragging_splitter {
                st.prefs.split_pos = get_x_lparam(lparam);
                st.layout_children();
                win32::invalidate(hwnd, false);
            }
            0
        }
        WM_LBUTTONUP => {
            if st.dragging_splitter {
                st.dragging_splitter = false;
                ReleaseCapture();
                st.prefs.save();
            }
            0
        }
        WM_NOTIFY => {
            let nmhdr = lparam as *const NMHDR;
            if !nmhdr.is_null() {
                let hdr = &*nmhdr;
                if hdr.hwndFrom == st.editor.0 && hdr.code == EN_SELCHANGE {
                    st.update_status_cursor_pos();
                }
            }
            0
        }
        WM_COMMAND => {
            let id_raw = loword(wparam);
            let code = hiword(wparam);

            if id_raw as usize == ControlId::Edit as usize {
                if code == EN_CHANGE && !st.suppress_change {
                    if st.editor.is_modified() {
                        st.maybe_live_wrap_current_line();
                        st.modified = true;
                        st.update_title();
                        st.update_status_cursor_pos();
                        win32::set_timer(hwnd, TimerId::Debounce, DEBOUNCE_MS);
                    }
                    return 0;
                } else if code == EN_VSCROLL {
                    st.sync_preview_scroll_to_editor();
                    return 0;
                }
            }

            if let Some(cmd) = MenuId::from_raw(id_raw) {
                st.handle_menu_command(cmd);
                0
            } else {
                DefWindowProcA(hwnd, msg, wparam, lparam)
            }
        }
        WM_TIMER => {
            match TimerId::from_raw(wparam) {
                Some(TimerId::Debounce) => {
                    win32::kill_timer(hwnd, TimerId::Debounce);
                    st.relayout_preview_from_editor();
                    st.sync_preview_scroll_to_editor();
                }
                Some(TimerId::Stress) => {
                    win32::kill_timer(hwnd, TimerId::Stress);
                    let n = st.cli.stress_edits;
                    if n > 0 {
                        st.run_stress_edits(n);
                    }
                    if let Some(save_path) = st.cli.save_test_path.clone() {
                        let _ = st.save_to_path(&save_path);
                    }
                }
                Some(TimerId::HeadlessExit) => {
                    win32::kill_timer(hwnd, TimerId::HeadlessExit);
                    st.headless_closing = true;
                    st.dump_metrics_file();
                    win32::post_close(hwnd);
                }
                None => {}
            }
            0
        }
        WM_SETFOCUS => {
            if st.prefs.view_mode != ViewMode::PreviewOnly {
                win32::set_focus(st.editor.0);
            } else {
                win32::set_focus(st.hwnd_preview);
            }
            0
        }
        WM_CLOSE => {
            if !st.headless_closing && !st.confirm_save_if_modified() {
                return 0;
            }
            st.dump_metrics_file();
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            st.backbuf.destroy(&mut st.gdi.stats);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcA(hwnd, msg, wparam, lparam),
    }
}

fn shortcut_for_keydown(wparam: WPARAM) -> Option<MenuId> {
    if wparam == VK_F11 {
        return Some(MenuId::ViewToggleF11);
    }
    if wparam == VK_F3 {
        return Some(MenuId::EditFindNext);
    }
    if win32::ctrl_is_down() {
        return match wparam as u8 {
            b'N' => Some(MenuId::FileNew),
            b'O' => Some(MenuId::FileOpen),
            b'S' => Some(MenuId::FileSave),
            b'Q' => Some(MenuId::FileExit),
            b'F' => Some(MenuId::EditFind),
            b'H' => Some(MenuId::EditReplace),
            b'R' => Some(MenuId::EditReflow),
            _ => None,
        };
    }
    None
}

pub fn run() {
    let cli = parse_cli();

    unsafe {
        let hinst = GetModuleHandleA(ptr::null());
        let icc = INITCOMMONCONTROLSEX {
            dwSize: core::mem::size_of::<INITCOMMONCONTROLSEX>() as DWORD,
            dwICC: ICC_BAR_CLASSES,
        };
        InitCommonControlsEx(&icc);

        let hriched_dll = LoadLibraryA(b"RICHED20.DLL\0".as_ptr());

        let hcursor_arrow = LoadCursorA(ptr::null_mut(), IDC_ARROW);
        let hcursor_sizewe = LoadCursorA(ptr::null_mut(), IDC_SIZEWE);
        let hicon = LoadIconA(ptr::null_mut(), IDI_APPLICATION);

        let main_cls = b"MD98_MAIN\0";
        let wc_main = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(main_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: hicon,
            hCursor: hcursor_arrow,
            hbrBackground: (COLOR_BTNFACE + 1) as HBRUSH,
            lpszMenuName: ptr::null(),
            lpszClassName: main_cls.as_ptr(),
        };
        RegisterClassA(&wc_main);

        let prev_cls = b"MD98_PREVIEW\0";
        let wc_prev = WNDCLASSA {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(preview_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: ptr::null_mut(),
            hCursor: hcursor_arrow,
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: prev_cls.as_ptr(),
        };
        RegisterClassA(&wc_prev);

        let hmenu = win32::build_main_menu();
        let hwnd_main = CreateWindowExA(
            0,
            main_cls.as_ptr(),
            b"md98 - Untitled\0".as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            780,
            560,
            ptr::null_mut(),
            hmenu,
            hinst,
            ptr::null_mut(),
        );
        if hwnd_main.is_null() {
            return;
        }

        let hwnd_edit = CreateWindowExA(
            WS_EX_CLIENTEDGE,
            b"RichEdit20A\0".as_ptr(),
            b"\0".as_ptr(),
            WS_CHILD
                | WS_VISIBLE
                | WS_VSCROLL
                | ES_MULTILINE
                | ES_AUTOVSCROLL
                | ES_WANTRETURN
                | ES_NOHIDESEL,
            0,
            0,
            380,
            500,
            hwnd_main,
            ControlId::Edit.as_hmenu(),
            hinst,
            ptr::null_mut(),
        );

        let hwnd_preview = CreateWindowExA(
            WS_EX_CLIENTEDGE,
            prev_cls.as_ptr(),
            b"\0".as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_HSCROLL,
            384,
            0,
            380,
            500,
            hwnd_main,
            ControlId::Preview.as_hmenu(),
            hinst,
            ptr::null_mut(),
        );

        let hwnd_status = CreateWindowExA(
            0,
            b"msctls_statusbar32\0".as_ptr(),
            b"Ready\0".as_ptr(),
            WS_CHILD | WS_VISIBLE | SBARS_SIZEGRIP | CCS_BOTTOM,
            0,
            0,
            0,
            0,
            hwnd_main,
            ControlId::Status.as_hmenu(),
            hinst,
            ptr::null_mut(),
        );

        let gdi = GdiCache::new();
        let editor = RichEdit(hwnd_edit);
        editor.configure(gdi.editor_font);

        let msg_find_replace = RegisterWindowMessageA(b"commdlg_FindReplace\0".as_ptr());
        let mut prefs = Prefs::load();
        if cli.dark_mode {
            prefs.dark_mode = true;
        }
        if let Some(w) = cli.wrap_width {
            prefs.wrap_width = w;
        }
        if cli.live_wrap {
            prefs.live_wrap = true;
        }
        if cli.reflow_on_save {
            prefs.reflow_on_save = true;
        }

        let mut state = Box::new(AppState {
            hwnd_main,
            editor,
            hwnd_preview,
            hwnd_status,
            hmenu,
            hcursor_sizewe,
            hriched_dll,
            msg_find_replace,
            hwnd_find: ptr::null_mut(),
            find_is_replace: false,
            find_fr: core::mem::zeroed(),
            find_buf: [0u8; 256],
            replace_buf: [0u8; 256],
            find_flags: FR_DOWN,
            prefs,
            gdi,
            backbuf: ViewportBackBuffer::new(),
            layout: DocumentLayout::new(),
            raw_ansi_buf: Vec::with_capacity(8192),
            utf8_buf: String::with_capacity(8192),
            current_file: None,
            modified: false,
            suppress_change: false,
            headless_closing: false,
            dragging_splitter: false,
            splitter_rect: RECT::default(),
            saved_edit_sel: CHARRANGE::default(),
            saved_edit_first_line: 0,
            has_saved_edit_pos: false,
            scroll_x: 0,
            scroll_y: 0,
            preview_w: 380,
            preview_h: 480,
            last_layout_ms: 0,
            cli,
            gdi_live_initial: 0,
            stress_completed: 0,
            save_test_bytes: 0,
            replace_count: 0,
        });

        let state_ptr = state.as_mut() as *mut AppState;
        SetWindowLongA(hwnd_main, GWL_USERDATA, state_ptr as isize);
        SetWindowLongA(hwnd_preview, GWL_USERDATA, state_ptr as isize);

        state.sync_all_menu_checks();

        ShowWindow(hwnd_main, SW_MAXIMIZE);
        state.layout_children();

        if let Some(fixture) = state.cli.fixture_path.clone() {
            let _ = state.open_path(&fixture);
        } else {
            state.load_markdown_text(
                "# Welcome to md98\n\nType **GitHub Flavored Markdown** on the left to preview live on *Windows 98 SE*.\n\n- [x] Native Win32 `RichEdit 2.0`\n- [x] Double-buffered GDI preview\n",
                None,
            );
        }

        UpdateWindow(hwnd_main);
        UpdateWindow(hwnd_preview);
        if state.prefs.view_mode == ViewMode::PreviewOnly {
            win32::set_focus(hwnd_preview);
        } else {
            win32::set_focus(hwnd_edit);
        }

        state.gdi_live_initial = state.gdi.stats.live_objects();

        if state.cli.stress_edits > 0 || state.cli.save_test_path.is_some() {
            win32::set_timer(hwnd_main, TimerId::Stress, 300);
        }
        if let Some(ms) = state.cli.headless_exit_ms {
            win32::set_timer(hwnd_main, TimerId::HeadlessExit, ms.max(500));
        }

        let mut msg: MSG = core::mem::zeroed();
        while GetMessageA(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            if !state.hwnd_find.is_null() && IsDialogMessageA(state.hwnd_find, &msg) != 0 {
                continue;
            }
            if msg.message == WM_KEYDOWN {
                if let Some(cmd) = shortcut_for_keydown(msg.wParam) {
                    win32::post_command(hwnd_main, cmd);
                    continue;
                }
            }
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }

        SetWindowLongA(hwnd_main, GWL_USERDATA, 0);
        SetWindowLongA(hwnd_preview, GWL_USERDATA, 0);
        let dll = state.hriched_dll;
        drop(state);
        if !dll.is_null() {
            FreeLibrary(dll);
        }
    }
}
