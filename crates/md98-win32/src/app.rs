//! Native Win32 SDI shell, RichEdit 2.0 editor, draggable splitter, file I/O,
//! debounced live preview, proportional scroll sync, and regression CLI flags.

#![cfg(windows)]

use crate::gdi::{paint_preview, GdiCache, ViewportBackBuffer, Win32Measurer};
use crate::win32_ffi::*;
use core::ptr;
use md98_core::{
    compute_live_wrap_break, cp1252_to_utf8, find_cp1252, is_line_in_fenced_code,
    layout_markdown_with_theme, normalize_to_crlf, normalize_to_lf, reflow_markdown,
    replace_all_text, utf8_to_cp1252, DocumentLayout, LayoutConfig, ThemePalette,
};

const ID_EDIT: usize = 1001;
const ID_PREVIEW: usize = 1002;
const ID_STATUS: usize = 1003;

const IDM_FILE_NEW: usize = 2001;
const IDM_FILE_OPEN: usize = 2002;
const IDM_FILE_SAVE: usize = 2003;
const IDM_FILE_SAVEAS: usize = 2004;
const IDM_FILE_EXIT: usize = 2005;

const IDM_EDIT_UNDO: usize = 2101;
const IDM_EDIT_CUT: usize = 2102;
const IDM_EDIT_COPY: usize = 2103;
const IDM_EDIT_PASTE: usize = 2104;
const IDM_EDIT_SELECTALL: usize = 2105;
const IDM_EDIT_FIND: usize = 2106;
const IDM_EDIT_FINDNEXT: usize = 2107;
const IDM_EDIT_REPLACE: usize = 2108;
const IDM_EDIT_REFLOW: usize = 2109;
const IDM_EDIT_LIVEWRAP: usize = 2110;
const IDM_EDIT_REFLOW_ONSAVE: usize = 2111;
const IDM_EDIT_WRAP_72: usize = 2112;
const IDM_EDIT_WRAP_80: usize = 2113;
const IDM_EDIT_WRAP_100: usize = 2114;

const IDM_VIEW_SPLIT: usize = 2201;
const IDM_VIEW_EDITOR: usize = 2202;
const IDM_VIEW_PREVIEW: usize = 2203;
const IDM_VIEW_SYNCSCROLL: usize = 2204;
const IDM_VIEW_DARKMODE: usize = 2205;
const IDM_VIEW_TOGGLE_F11: usize = 2206;

const IDM_HELP_ABOUT: usize = 2301;

const TIMER_DEBOUNCE: usize = 1;
const TIMER_HEADLESS_EXIT: usize = 2;
const TIMER_STRESS: usize = 3;

const DEBOUNCE_MS: UINT = 250;
const SPLITTER_WIDTH: i32 = 4;
const REG_SUBKEY: &[u8] = b"Software\\md98\0";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Split,
    EditorOnly,
    PreviewOnly,
}

impl ViewMode {
    fn to_dword(self) -> DWORD {
        match self {
            ViewMode::Split => 0,
            ViewMode::EditorOnly => 1,
            ViewMode::PreviewOnly => 2,
        }
    }

    fn from_dword(v: DWORD) -> Self {
        match v {
            1 => ViewMode::EditorOnly,
            2 => ViewMode::PreviewOnly,
            _ => ViewMode::Split,
        }
    }
}

struct Prefs {
    view_mode: ViewMode,
    dark_mode: bool,
    sync_scroll: bool,
    wrap_width: usize,
    live_wrap: bool,
    reflow_on_save: bool,
    split_pos: i32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            view_mode: ViewMode::Split,
            dark_mode: false,
            sync_scroll: true,
            wrap_width: 80,
            live_wrap: false,
            reflow_on_save: false,
            split_pos: 360,
        }
    }
}

fn reg_read_dword(hkey: HKEY, name: &[u8]) -> Option<DWORD> {
    let mut reg_type: DWORD = 0;
    let mut val: DWORD = 0;
    let mut sz: DWORD = core::mem::size_of::<DWORD>() as DWORD;
    let status = unsafe {
        RegQueryValueExA(
            hkey,
            name.as_ptr(),
            ptr::null_mut(),
            &mut reg_type,
            &mut val as *mut DWORD as *mut u8,
            &mut sz,
        )
    };
    if status == ERROR_SUCCESS && reg_type == REG_DWORD && sz == 4 {
        Some(val)
    } else {
        None
    }
}

fn reg_write_dword(hkey: HKEY, name: &[u8], val: DWORD) {
    unsafe {
        let _ = RegSetValueExA(
            hkey,
            name.as_ptr(),
            0,
            REG_DWORD,
            &val as *const DWORD as *const u8,
            core::mem::size_of::<DWORD>() as DWORD,
        );
    }
}

fn load_prefs_from_registry() -> Prefs {
    let mut prefs = Prefs::default();
    let mut hkey: HKEY = ptr::null_mut();
    let status = unsafe { RegOpenKeyExA(HKEY_CURRENT_USER, REG_SUBKEY.as_ptr(), 0, KEY_READ, &mut hkey) };
    if status != ERROR_SUCCESS || hkey.is_null() {
        return prefs;
    }
    if let Some(v) = reg_read_dword(hkey, b"ViewMode\0") {
        prefs.view_mode = ViewMode::from_dword(v);
    }
    if let Some(v) = reg_read_dword(hkey, b"DarkMode\0") {
        prefs.dark_mode = v != 0;
    }
    if let Some(v) = reg_read_dword(hkey, b"SyncScroll\0") {
        prefs.sync_scroll = v != 0;
    }
    if let Some(v) = reg_read_dword(hkey, b"WrapWidth\0") {
        let w = v as usize;
        if matches!(w, 72 | 80 | 100) {
            prefs.wrap_width = w;
        }
    }
    if let Some(v) = reg_read_dword(hkey, b"LiveWrap\0") {
        prefs.live_wrap = v != 0;
    }
    if let Some(v) = reg_read_dword(hkey, b"ReflowOnSave\0") {
        prefs.reflow_on_save = v != 0;
    }
    if let Some(v) = reg_read_dword(hkey, b"SplitPos\0") {
        if (100..=2000).contains(&v) {
            prefs.split_pos = v as i32;
        }
    }
    unsafe {
        RegCloseKey(hkey);
    }
    prefs
}

fn save_prefs_to_registry(prefs: &Prefs) {
    let mut hkey: HKEY = ptr::null_mut();
    let mut disp: DWORD = 0;
    let status = unsafe {
        RegCreateKeyExA(
            HKEY_CURRENT_USER,
            REG_SUBKEY.as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            ptr::null_mut(),
            &mut hkey,
            &mut disp,
        )
    };
    if status != ERROR_SUCCESS || hkey.is_null() {
        return;
    }
    reg_write_dword(hkey, b"ViewMode\0", prefs.view_mode.to_dword());
    reg_write_dword(hkey, b"DarkMode\0", if prefs.dark_mode { 1 } else { 0 });
    reg_write_dword(hkey, b"SyncScroll\0", if prefs.sync_scroll { 1 } else { 0 });
    reg_write_dword(hkey, b"WrapWidth\0", prefs.wrap_width as DWORD);
    reg_write_dword(hkey, b"LiveWrap\0", if prefs.live_wrap { 1 } else { 0 });
    reg_write_dword(hkey, b"ReflowOnSave\0", if prefs.reflow_on_save { 1 } else { 0 });
    if prefs.split_pos > 0 {
        reg_write_dword(hkey, b"SplitPos\0", prefs.split_pos as DWORD);
    }
    unsafe {
        RegCloseKey(hkey);
    }
}

#[derive(Default)]
struct CliOptions {
    fixture_path: Option<String>,
    save_test_path: Option<String>,
    headless_exit_ms: Option<u32>,
    stress_edits: u32,
    dump_metrics: bool,
    dark_mode: bool,
    wrap_width: Option<usize>,
    live_wrap: bool,
    reflow_on_save: bool,
}

struct AppState {
    hwnd_main: HWND,
    hwnd_edit: HWND,
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

    // Reflow / Live Auto-Wrap state (72, 80, or 100 chars)
    wrap_width: usize,
    live_wrap: bool,
    reflow_on_save: bool,

    gdi: GdiCache,
    backbuf: ViewportBackBuffer,
    layout: DocumentLayout,

    // Reusable scratch buffers to minimize heap churn on 40 MB RAM
    raw_ansi_buf: Vec<u8>,
    utf8_buf: String,

    current_file: Option<String>,
    modified: bool,
    suppress_change: bool,

    dark_mode: bool,
    view_mode: ViewMode,
    sync_scroll: bool,
    split_pos: i32,
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

/// Convert UTF-8 string to NUL-terminated Windows-1252 (CP1252) bytes for Win32 `*A` APIs.
fn cstr(s: &str) -> Vec<u8> {
    let mut v = utf8_to_cp1252(s);
    v.push(0);
    v
}

fn parse_cli() -> CliOptions {
    let mut opts = CliOptions::default();
    let cmd = unsafe {
        let p = GetCommandLineA();
        if p.is_null() {
            return opts;
        }
        let mut len = 0usize;
        while *p.add(len) != 0 && len < 4096 {
            len += 1;
        }
        let slice = core::slice::from_raw_parts(p, len);
        cp1252_to_utf8(slice)
    };

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
                if let Ok(w) = tokens[i + 1].parse::<usize>() {
                    if matches!(w, 72 | 80 | 100) {
                        opts.wrap_width = Some(w);
                    }
                }
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

fn read_file_bytes(path: &str) -> Option<Vec<u8>> {
    let p = cstr(path);
    unsafe {
        let h = CreateFileA(
            p.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return None;
        }
        let sz = GetFileSize(h, ptr::null_mut());
        if sz == 0xFFFF_FFFF || sz > 8 * 1024 * 1024 {
            CloseHandle(h);
            return None;
        }
        let mut buf = vec![0u8; sz as usize];
        let mut total = 0usize;
        while total < buf.len() {
            let mut n: DWORD = 0;
            let ok = ReadFile(
                h,
                buf.as_mut_ptr().add(total),
                (buf.len() - total) as DWORD,
                &mut n,
                ptr::null_mut(),
            );
            if ok == 0 || n == 0 {
                break;
            }
            total += n as usize;
        }
        CloseHandle(h);
        buf.truncate(total);
        Some(buf)
    }
}

fn write_file_bytes(path: &str, data: &[u8]) -> bool {
    let p = cstr(path);
    unsafe {
        let h = CreateFileA(
            p.as_ptr(),
            GENERIC_WRITE,
            0,
            ptr::null_mut(),
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut offset = 0usize;
        let mut ok_all = true;
        while offset < data.len() {
            let mut written: DWORD = 0;
            let ok = WriteFile(
                h,
                data.as_ptr().add(offset),
                (data.len() - offset) as DWORD,
                &mut written,
                ptr::null_mut(),
            );
            if ok == 0 || written == 0 {
                ok_all = false;
                break;
            }
            offset += written as usize;
        }
        CloseHandle(h);
        ok_all
    }
}

impl AppState {
    fn current_theme(&self) -> ThemePalette {
        if self.dark_mode {
            ThemePalette::DARK
        } else {
            ThemePalette::LIGHT
        }
    }

    fn save_editor_cursor_and_scroll(&mut self) {
        unsafe {
            let mut cr = CHARRANGE::default();
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
            let first_line = SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) as i32;
            self.saved_edit_sel = cr;
            self.saved_edit_first_line = first_line;
            self.has_saved_edit_pos = true;
        }
    }

    fn restore_editor_cursor_and_scroll(&self) {
        if !self.has_saved_edit_pos {
            return;
        }
        unsafe {
            let cr = self.saved_edit_sel;
            SendMessageA(self.hwnd_edit, EM_EXSETSEL, 0, &cr as *const _ as LPARAM);
            let cur_first = SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) as i32;
            let delta = self.saved_edit_first_line - cur_first;
            if delta != 0 {
                SendMessageA(self.hwnd_edit, EM_LINESCROLL, 0, delta as LPARAM);
            }
        }
    }

    fn apply_editor_theme(&mut self) {
        let theme = self.current_theme();
        let prev_suppress = self.suppress_change;
        self.suppress_change = true;
        unsafe {
            let was_modified = SendMessageA(self.hwnd_edit, EM_GETMODIFY, 0, 0) as WPARAM;
            let mut cr = CHARRANGE::default();
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
            let first_line = SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) as i32;

            SendMessageA(self.hwnd_edit, EM_SETBKGNDCOLOR, 0, theme.bg as LPARAM);

            let mut cf: CHARFORMATA = core::mem::zeroed();
            cf.cbSize = core::mem::size_of::<CHARFORMATA>() as UINT;
            cf.dwMask = CFM_COLOR;
            cf.crTextColor = theme.text;
            SendMessageA(self.hwnd_edit, EM_SETCHARFORMAT, SCF_ALL, &cf as *const _ as LPARAM);
            SendMessageA(self.hwnd_edit, EM_SETCHARFORMAT, SCF_DEFAULT, &cf as *const _ as LPARAM);

            SendMessageA(self.hwnd_edit, EM_EXSETSEL, 0, &cr as *const _ as LPARAM);
            let cur_first = SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) as i32;
            if cur_first != first_line {
                SendMessageA(self.hwnd_edit, EM_LINESCROLL, 0, (first_line - cur_first) as LPARAM);
            }
            SendMessageA(self.hwnd_edit, EM_SETMODIFY, was_modified, 0);
            InvalidateRect(self.hwnd_edit, ptr::null(), 1);
        }
        self.suppress_change = prev_suppress;
    }

    fn save_prefs(&self) {
        let prefs = Prefs {
            view_mode: self.view_mode,
            dark_mode: self.dark_mode,
            sync_scroll: self.sync_scroll,
            wrap_width: self.wrap_width,
            live_wrap: self.live_wrap,
            reflow_on_save: self.reflow_on_save,
            split_pos: self.split_pos,
        };
        save_prefs_to_registry(&prefs);
    }

    fn sync_all_menu_checks(&self) {
        unsafe {
            CheckMenuItem(
                self.hmenu,
                IDM_VIEW_SPLIT as UINT,
                MF_BYCOMMAND | if self.view_mode == ViewMode::Split { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_VIEW_EDITOR as UINT,
                MF_BYCOMMAND | if self.view_mode == ViewMode::EditorOnly { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_VIEW_PREVIEW as UINT,
                MF_BYCOMMAND | if self.view_mode == ViewMode::PreviewOnly { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_VIEW_SYNCSCROLL as UINT,
                MF_BYCOMMAND | if self.sync_scroll { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_VIEW_DARKMODE as UINT,
                MF_BYCOMMAND | if self.dark_mode { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_WRAP_72 as UINT,
                MF_BYCOMMAND | if self.wrap_width == 72 { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_WRAP_80 as UINT,
                MF_BYCOMMAND | if self.wrap_width == 80 { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_WRAP_100 as UINT,
                MF_BYCOMMAND | if self.wrap_width == 100 { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_LIVEWRAP as UINT,
                MF_BYCOMMAND | if self.live_wrap { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_REFLOW_ONSAVE as UINT,
                MF_BYCOMMAND | if self.reflow_on_save { MF_CHECKED } else { MF_UNCHECKED },
            );
        }
    }

    fn toggle_dark_mode(&mut self) {
        self.dark_mode = !self.dark_mode;
        unsafe {
            CheckMenuItem(
                self.hmenu,
                IDM_VIEW_DARKMODE as UINT,
                MF_BYCOMMAND | if self.dark_mode { MF_CHECKED } else { MF_UNCHECKED },
            );
        }
        self.save_prefs();
        self.apply_editor_theme();
        self.relayout_preview_from_cached_utf8();
        unsafe {
            InvalidateRect(self.hwnd_main, ptr::null(), 1);
        }
    }

    fn set_view_mode(&mut self, new_mode: ViewMode) {
        if new_mode == self.view_mode {
            return;
        }
        let prev_mode = self.view_mode;

        // When leaving a mode where the editor was active, save its caret + scroll
        // and flush any pending debounced edits so preview reflects the latest text.
        if prev_mode != ViewMode::PreviewOnly {
            self.save_editor_cursor_and_scroll();
            unsafe {
                KillTimer(self.hwnd_main, TIMER_DEBOUNCE);
            }
            self.read_editor_to_utf8();
        }

        self.view_mode = new_mode;
        unsafe {
            CheckMenuItem(
                self.hmenu,
                IDM_VIEW_SPLIT as UINT,
                MF_BYCOMMAND | if self.view_mode == ViewMode::Split { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_VIEW_EDITOR as UINT,
                MF_BYCOMMAND | if self.view_mode == ViewMode::EditorOnly { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_VIEW_PREVIEW as UINT,
                MF_BYCOMMAND | if self.view_mode == ViewMode::PreviewOnly { MF_CHECKED } else { MF_UNCHECKED },
            );
        }

        self.layout_children();
        self.save_prefs();

        match self.view_mode {
            ViewMode::EditorOnly => {
                self.restore_editor_cursor_and_scroll();
                unsafe {
                    SetFocus(self.hwnd_edit);
                }
                self.update_status_bar();
            }
            ViewMode::PreviewOnly => {
                if prev_mode == ViewMode::EditorOnly {
                    // Refresh preview content in case text changed while preview was hidden,
                    // keeping preview scroll_y intact (same viewport width).
                    self.relayout_preview_from_cached_utf8();
                }
                unsafe {
                    SetFocus(self.hwnd_preview);
                }
            }
            ViewMode::Split => {
                if prev_mode == ViewMode::PreviewOnly {
                    self.restore_editor_cursor_and_scroll();
                }
                self.relayout_preview_from_cached_utf8();
                unsafe {
                    SetFocus(self.hwnd_edit);
                }
            }
        }
    }

    fn toggle_preview_editor_f11(&mut self) {
        if self.view_mode == ViewMode::PreviewOnly {
            self.set_view_mode(ViewMode::EditorOnly);
        } else {
            self.set_view_mode(ViewMode::PreviewOnly);
        }
    }

    fn set_wrap_width(&mut self, width: usize) {
        self.wrap_width = width;
        unsafe {
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_WRAP_72 as UINT,
                MF_BYCOMMAND | if width == 72 { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_WRAP_80 as UINT,
                MF_BYCOMMAND | if width == 80 { MF_CHECKED } else { MF_UNCHECKED },
            );
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_WRAP_100 as UINT,
                MF_BYCOMMAND | if width == 100 { MF_CHECKED } else { MF_UNCHECKED },
            );
        }
        self.save_prefs();
        self.update_status_bar();
    }

    fn toggle_live_wrap(&mut self) {
        self.live_wrap = !self.live_wrap;
        unsafe {
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_LIVEWRAP as UINT,
                MF_BYCOMMAND | if self.live_wrap { MF_CHECKED } else { MF_UNCHECKED },
            );
        }
        self.save_prefs();
        self.update_status_bar();
    }

    fn toggle_reflow_on_save(&mut self) {
        self.reflow_on_save = !self.reflow_on_save;
        unsafe {
            CheckMenuItem(
                self.hmenu,
                IDM_EDIT_REFLOW_ONSAVE as UINT,
                MF_BYCOMMAND | if self.reflow_on_save { MF_CHECKED } else { MF_UNCHECKED },
            );
        }
        self.save_prefs();
        self.update_status_bar();
    }

    /// Reflow the entire document in-place while preserving caret & scroll position.
    fn reflow_full_document(&mut self) -> bool {
        let mut cr = CHARRANGE::default();
        let first_line = unsafe {
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
            SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) as i32
        };
        self.read_editor_to_utf8();
        let reflowed = reflow_markdown(&self.utf8_buf, self.wrap_width);
        if reflowed == self.utf8_buf {
            return false;
        }
        let crlf = normalize_to_crlf(&reflowed);
        let mut ansi = utf8_to_cp1252(&crlf);
        let max_cp = reflowed.chars().count() as i32;
        ansi.push(0);
        self.suppress_change = true;
        unsafe {
            SendMessageA(self.hwnd_edit, WM_SETTEXT, 0, ansi.as_ptr() as LPARAM);
            SendMessageA(self.hwnd_edit, EM_SETMODIFY, 1, 0);
        }
        self.apply_editor_theme();
        let restored = CHARRANGE {
            cpMin: cr.cpMin.clamp(0, max_cp),
            cpMax: cr.cpMin.clamp(0, max_cp),
        };
        unsafe {
            SendMessageA(self.hwnd_edit, EM_EXSETSEL, 0, &restored as *const _ as LPARAM);
            let cur_first = SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) as i32;
            if cur_first != first_line {
                SendMessageA(self.hwnd_edit, EM_LINESCROLL, 0, (first_line - cur_first) as LPARAM);
            }
        }
        self.suppress_change = false;
        self.utf8_buf = reflowed;
        self.relayout_preview_from_cached_utf8();
        true
    }

    /// On-demand reflow: reflows current selection if non-empty, otherwise the entire document.
    fn reflow_editor_text(&mut self) {
        let mut cr = CHARRANGE::default();
        unsafe {
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
        }

        if cr.cpMax > cr.cpMin {
            // Reflow selected range (undoable via EM_REPLACESEL)
            let sel_len = (cr.cpMax - cr.cpMin) as usize;
            let mut sel_buf = vec![0u8; sel_len + 4];
            let copied = unsafe {
                SendMessageA(self.hwnd_edit, EM_GETSELTEXT, 0, sel_buf.as_mut_ptr() as LPARAM)
            } as usize;
            let actual = copied.min(sel_buf.len());
            let sel_utf8 = normalize_to_lf(&cp1252_to_utf8(&sel_buf[..actual]));
            let reflowed = reflow_markdown(&sel_utf8, self.wrap_width);
            if reflowed != sel_utf8 {
                let crlf = normalize_to_crlf(&reflowed);
                let mut ansi = utf8_to_cp1252(&crlf);
                ansi.push(0);
                self.suppress_change = true;
                unsafe {
                    SendMessageA(self.hwnd_edit, EM_REPLACESEL, 1, ansi.as_ptr() as LPARAM);
                    SendMessageA(self.hwnd_edit, EM_SETMODIFY, 1, 0);
                }
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

    /// Live auto-wrap: when enabled and current line exceeds `wrap_width`, breaks cleanly at word boundary.
    fn maybe_live_wrap_current_line(&mut self) {
        if !self.live_wrap || self.suppress_change {
            return;
        }
        let mut cr = CHARRANGE::default();
        unsafe {
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
        }
        if cr.cpMin != cr.cpMax {
            return;
        }
        self.read_editor_to_utf8();
        // Locate the hard-newline paragraph containing the caret directly in CP1252-LF offsets
        // (avoiding EM_EXLINEFROMCHAR which counts visually soft-wrapped display lines).
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
            compute_live_wrap_break(&line_str, self.wrap_width, in_fence)
        else {
            return;
        };

        let break_cp = (hard_start + break_char_off) as i32;
        if cr.cpMin <= break_cp {
            return;
        }

        // Replace the single space at `break_cp` with `\r\n` + hanging prefix
        let cont_chars = insert_str.chars().count().saturating_sub(2) as i32;
        let mut repl_ansi = utf8_to_cp1252(&insert_str);
        repl_ansi.push(0);
        let target = CHARRANGE {
            cpMin: break_cp,
            cpMax: break_cp + 1,
        };
        let new_caret = CHARRANGE {
            cpMin: cr.cpMin + cont_chars,
            cpMax: cr.cpMin + cont_chars,
        };
        self.suppress_change = true;
        unsafe {
            SendMessageA(self.hwnd_edit, EM_EXSETSEL, 0, &target as *const _ as LPARAM);
            SendMessageA(self.hwnd_edit, EM_REPLACESEL, 1, repl_ansi.as_ptr() as LPARAM);
            SendMessageA(self.hwnd_edit, EM_EXSETSEL, 0, &new_caret as *const _ as LPARAM);
            SendMessageA(self.hwnd_edit, EM_SCROLLCARET, 0, 0);
        }
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

    fn open_find_dialog(&mut self, replace: bool) {
        if self.view_mode == ViewMode::PreviewOnly {
            self.set_view_mode(ViewMode::EditorOnly);
        }
        if !self.hwnd_find.is_null() {
            if self.find_is_replace == replace {
                unsafe {
                    SetFocus(self.hwnd_find);
                }
                return;
            }
            unsafe {
                DestroyWindow(self.hwnd_find);
            }
            self.hwnd_find = ptr::null_mut();
        }

        // Pre-fill Find box with current single-line selection if non-empty
        let mut cr = CHARRANGE::default();
        unsafe {
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
        }
        if cr.cpMax > cr.cpMin && (cr.cpMax - cr.cpMin) < 128 {
            let mut sel = vec![0u8; (cr.cpMax - cr.cpMin) as usize + 4];
            let copied = unsafe {
                SendMessageA(self.hwnd_edit, EM_GETSELTEXT, 0, sel.as_mut_ptr() as LPARAM)
            } as usize;
            let slice = &sel[..copied.min(sel.len())];
            if !slice.contains(&b'\r') && !slice.contains(&b'\n') && !slice.is_empty() {
                let n = slice.len().min(self.find_buf.len() - 1);
                self.find_buf[..n].copy_from_slice(&slice[..n]);
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

        let mut cr = CHARRANGE::default();
        unsafe {
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
        }
        let down = (self.find_flags & FR_DOWN) != 0;
        let match_case = (self.find_flags & FR_MATCHCASE) != 0;
        let whole_word = (self.find_flags & FR_WHOLEWORD) != 0;
        let start_pos = if down {
            cr.cpMax.max(0) as usize
        } else {
            cr.cpMin.max(0) as usize
        };

        if let Some((m_start, m_end)) = find_cp1252(&hay, &ndl, start_pos, down, match_case, whole_word) {
            let found = CHARRANGE {
                cpMin: m_start as i32,
                cpMax: m_end as i32,
            };
            unsafe {
                SendMessageA(self.hwnd_edit, EM_EXSETSEL, 0, &found as *const _ as LPARAM);
                SendMessageA(self.hwnd_edit, EM_SCROLLCARET, 0, 0);
            }
            self.update_status_bar();
            true
        } else {
            if notify_not_found {
                let owner = if !self.hwnd_find.is_null() {
                    self.hwnd_find
                } else {
                    self.hwnd_main
                };
                let msg = cstr(&format!("Cannot find \"{}\".", needle_utf8));
                let cap = cstr("md98");
                unsafe {
                    MessageBoxA(owner, msg.as_ptr(), cap.as_ptr(), MB_OK | MB_ICONINFORMATION);
                }
            }
            false
        }
    }

    fn do_replace_current(&mut self) {
        let needle_utf8 = self.find_what_str();
        if needle_utf8.is_empty() {
            return;
        }
        let mut cr = CHARRANGE::default();
        unsafe {
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
        }
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
            let mut repl_ansi = utf8_to_cp1252(&normalize_to_crlf(&repl_utf8));
            repl_ansi.push(0);
            self.suppress_change = true;
            unsafe {
                SendMessageA(self.hwnd_edit, EM_REPLACESEL, 1, repl_ansi.as_ptr() as LPARAM);
                SendMessageA(self.hwnd_edit, EM_SETMODIFY, 1, 0);
            }
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

        let first_line = unsafe { SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) } as i32;
        self.read_editor_to_utf8();
        let (new_utf8, count) =
            replace_all_text(&self.utf8_buf, &needle_utf8, &repl_utf8, match_case, whole_word);

        if count > 0 {
            let crlf = normalize_to_crlf(&new_utf8);
            let mut ansi = utf8_to_cp1252(&crlf);
            ansi.push(0);
            self.suppress_change = true;
            unsafe {
                SendMessageA(self.hwnd_edit, WM_SETTEXT, 0, ansi.as_ptr() as LPARAM);
                SendMessageA(self.hwnd_edit, EM_SETMODIFY, 1, 0);
            }
            self.apply_editor_theme();
            unsafe {
                let cur_first = SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) as i32;
                if cur_first != first_line {
                    SendMessageA(self.hwnd_edit, EM_LINESCROLL, 0, (first_line - cur_first) as LPARAM);
                }
            }
            self.suppress_change = false;
            self.modified = true;
            self.replace_count += count;
            self.update_title();
            self.relayout_preview_from_editor();
        }

        let owner = if !self.hwnd_find.is_null() {
            self.hwnd_find
        } else {
            self.hwnd_main
        };
        let msg = cstr(&format!("Replaced {} occurrence(s).", count));
        let cap = cstr("md98");
        unsafe {
            MessageBoxA(owner, msg.as_ptr(), cap.as_ptr(), MB_OK | MB_ICONINFORMATION);
        }
    }

    fn handle_find_replace_msg(&mut self) {
        let flags = self.find_fr.Flags;
        // Persist user's direction / match-case / whole-word preferences
        self.find_flags = flags & (FR_DOWN | FR_MATCHCASE | FR_WHOLEWORD);

        if (flags & FR_DIALOGTERM) != 0 {
            self.hwnd_find = ptr::null_mut();
            unsafe {
                SetFocus(self.hwnd_edit);
            }
            return;
        }
        if (flags & FR_FINDNEXT) != 0 {
            let _ = self.do_find_next(true);
        } else if (flags & FR_REPLACE) != 0 {
            self.do_replace_current();
        } else if (flags & FR_REPLACEALL) != 0 {
            self.do_replace_all();
        }
    }

    fn update_title(&self) {
        let name = self.current_file.as_deref().unwrap_or("Untitled");
        let star = if self.modified { "*" } else { "" };
        let title = cstr(&format!("md98 - {}{}", name, star));
        unsafe {
            SetWindowTextA(self.hwnd_main, title.as_ptr());
        }
    }

    fn update_status_bar(&self) {
        if self.hwnd_status.is_null() {
            return;
        }
        // Compute 1-based Ln / Col from RichEdit
        let (ln, col) = unsafe {
            let mut cr = CHARRANGE { cpMin: 0, cpMax: 0 };
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
            let line = SendMessageA(self.hwnd_edit, EM_EXLINEFROMCHAR, 0, cr.cpMin as LPARAM) as i32;
            let line_start = SendMessageA(self.hwnd_edit, EM_LINEINDEX, line as WPARAM, 0) as i32;
            let col = (cr.cpMin - line_start).max(0) + 1;
            (line + 1, col)
        };

        let p0 = cstr(self.current_file.as_deref().unwrap_or("Ready"));
        let p1 = cstr(&format!("Ln {}, Col {}", ln, col));
        let p2 = cstr(&format!("{} bytes", self.utf8_buf.len()));
        let mut wrap_flags = String::new();
        if self.live_wrap {
            wrap_flags.push('L');
        }
        if self.reflow_on_save {
            wrap_flags.push('S');
        }
        let wrap_label = format!("Wrap: {}{}", self.wrap_width, wrap_flags);
        let p3 = cstr(&format!("{} | {} ms", wrap_label, self.last_layout_ms));
        let p4 = cstr(&format!("GDI: {}", self.gdi.stats.live_objects()));

        unsafe {
            SendMessageA(self.hwnd_status, SB_SETTEXTA, 0, p0.as_ptr() as LPARAM);
            SendMessageA(self.hwnd_status, SB_SETTEXTA, 1, p1.as_ptr() as LPARAM);
            SendMessageA(self.hwnd_status, SB_SETTEXTA, 2, p2.as_ptr() as LPARAM);
            SendMessageA(self.hwnd_status, SB_SETTEXTA, 3, p3.as_ptr() as LPARAM);
            SendMessageA(self.hwnd_status, SB_SETTEXTA, 4, p4.as_ptr() as LPARAM);
        }
    }

    fn layout_children(&mut self) {
        let mut rc = RECT::default();
        unsafe {
            GetClientRect(self.hwnd_main, &mut rc);
        }
        let total_w = rc.right - rc.left;
        let total_h = rc.bottom - rc.top;

        // Resize status bar first and query its height
        let mut status_h = 0;
        if !self.hwnd_status.is_null() {
            unsafe {
                SendMessageA(self.hwnd_status, WM_SIZE, 0, 0);
                let mut src = RECT::default();
                GetClientRect(self.hwnd_status, &mut src);
                status_h = src.bottom - src.top;
                let parts: [i32; 5] = [
                    (total_w - 395).max(120),
                    (total_w - 300).max(200),
                    (total_w - 205).max(280),
                    (total_w - 75).max(380),
                    -1,
                ];
                SendMessageA(self.hwnd_status, SB_SETPARTS, 5, parts.as_ptr() as LPARAM);
            }
        }

        let body_h = (total_h - status_h).max(1);
        if self.split_pos <= 0 {
            self.split_pos = (total_w / 2).max(120);
        }
        let max_split = (total_w - SPLITTER_WIDTH - 100).max(100);
        self.split_pos = self.split_pos.clamp(100, max_split);

        match self.view_mode {
            ViewMode::Split => {
                let left_w = self.split_pos;
                let right_x = left_w + SPLITTER_WIDTH;
                let right_w = (total_w - right_x).max(1);
                self.splitter_rect = RECT {
                    left: left_w,
                    top: 0,
                    right: right_x,
                    bottom: body_h,
                };
                unsafe {
                    MoveWindow(self.hwnd_edit, 0, 0, left_w, body_h, 1);
                    ShowWindow(self.hwnd_edit, SW_SHOW);
                    MoveWindow(self.hwnd_preview, right_x, 0, right_w, body_h, 1);
                    ShowWindow(self.hwnd_preview, SW_SHOW);
                }
            }
            ViewMode::EditorOnly => {
                self.splitter_rect = RECT::default();
                unsafe {
                    MoveWindow(self.hwnd_edit, 0, 0, total_w, body_h, 1);
                    ShowWindow(self.hwnd_edit, SW_SHOW);
                    ShowWindow(self.hwnd_preview, 0);
                }
            }
            ViewMode::PreviewOnly => {
                self.splitter_rect = RECT::default();
                unsafe {
                    ShowWindow(self.hwnd_edit, 0);
                    MoveWindow(self.hwnd_preview, 0, 0, total_w, body_h, 1);
                    ShowWindow(self.hwnd_preview, SW_SHOW);
                }
            }
        }

        // Measure actual preview client area only when preview is visible so hiding
        // it in EditorOnly preserves its exact width and pixel scroll offset.
        if self.view_mode != ViewMode::EditorOnly {
            let mut prc = RECT::default();
            unsafe {
                GetClientRect(self.hwnd_preview, &mut prc);
            }
            let new_pw = (prc.right - prc.left).max(1);
            let new_ph = (prc.bottom - prc.top).max(1);
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
                    unsafe {
                        InvalidateRect(self.hwnd_preview, ptr::null(), 0);
                    }
                }
            } else {
                self.update_preview_scrollbars();
            }
        }
    }

    fn read_editor_to_utf8(&mut self) {
        let len = unsafe { SendMessageA(self.hwnd_edit, WM_GETTEXTLENGTH, 0, 0) } as usize;
        self.raw_ansi_buf.clear();
        self.raw_ansi_buf.resize(len + 1, 0);
        let copied = unsafe {
            SendMessageA(
                self.hwnd_edit,
                WM_GETTEXT,
                self.raw_ansi_buf.len(),
                self.raw_ansi_buf.as_mut_ptr() as LPARAM,
            )
        } as usize;
        let actual = copied.min(len);
        let decoded = cp1252_to_utf8(&self.raw_ansi_buf[..actual]);
        self.utf8_buf = normalize_to_lf(&decoded);
    }

    fn relayout_preview_from_editor(&mut self) {
        self.read_editor_to_utf8();
        self.relayout_preview_from_cached_utf8();
    }

    fn relayout_preview_from_cached_utf8(&mut self) {
        let t0 = unsafe { GetTickCount() };
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
        self.last_layout_ms = unsafe { GetTickCount().wrapping_sub(t0) };
        self.update_preview_scrollbars();
        unsafe {
            InvalidateRect(self.hwnd_preview, ptr::null(), 0);
        }
        self.update_status_bar();
    }

    fn update_preview_scrollbars(&mut self) {
        let max_y = (self.layout.content_height - self.preview_h).max(0);
        self.scroll_y = self.scroll_y.clamp(0, max_y);
        let si_v = SCROLLINFO {
            cbSize: core::mem::size_of::<SCROLLINFO>() as UINT,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: self.layout.content_height.max(self.preview_h),
            nPage: self.preview_h.max(1) as UINT,
            nPos: self.scroll_y,
            nTrackPos: 0,
        };
        unsafe {
            SetScrollInfo(self.hwnd_preview, SB_VERT, &si_v, 1);
        }

        let max_x = (self.layout.content_width - self.preview_w).max(0);
        self.scroll_x = self.scroll_x.clamp(0, max_x);
        let si_h = SCROLLINFO {
            cbSize: core::mem::size_of::<SCROLLINFO>() as UINT,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: self.layout.content_width.max(self.preview_w),
            nPage: self.preview_w.max(1) as UINT,
            nPos: self.scroll_x,
            nTrackPos: 0,
        };
        unsafe {
            SetScrollInfo(self.hwnd_preview, SB_HORZ, &si_h, 1);
        }
    }

    fn sync_preview_scroll_to_editor(&mut self) {
        if !self.sync_scroll || self.view_mode != ViewMode::Split {
            return;
        }
        let first_line = unsafe { SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) } as i32;
        let line_count = unsafe { SendMessageA(self.hwnd_edit, EM_GETLINECOUNT, 0, 0) } as i32;
        let max_scroll = (self.layout.content_height - self.preview_h).max(0);
        if line_count > 1 && max_scroll > 0 {
            let target_y = ((first_line as i64 * max_scroll as i64) / (line_count - 1) as i64) as i32;
            let clamped = target_y.clamp(0, max_scroll);
            if clamped != self.scroll_y {
                self.scroll_y = clamped;
                self.update_preview_scrollbars();
                unsafe {
                    InvalidateRect(self.hwnd_preview, ptr::null(), 0);
                }
            }
        }
    }

    fn load_markdown_text(&mut self, utf8: &str, file_path: Option<String>) {
        let crlf = normalize_to_crlf(utf8);
        let mut ansi = utf8_to_cp1252(&crlf);
        ansi.push(0);
        self.suppress_change = true;
        unsafe {
            SendMessageA(self.hwnd_edit, WM_SETTEXT, 0, ansi.as_ptr() as LPARAM);
            SendMessageA(self.hwnd_edit, EM_SETMODIFY, 0, 0);
        }
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
        if let Some(bytes) = read_file_bytes(path) {
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
        if self.reflow_on_save {
            let _ = self.reflow_full_document();
        } else {
            self.read_editor_to_utf8();
        }
        if write_file_bytes(path, self.utf8_buf.as_bytes()) {
            self.current_file = Some(path.to_string());
            self.modified = false;
            unsafe {
                SendMessageA(self.hwnd_edit, EM_SETMODIFY, 0, 0);
            }
            self.save_test_bytes = self.utf8_buf.len();
            self.update_title();
            self.update_status_bar();
            true
        } else {
            false
        }
    }

    fn open_dialog(&mut self) {
        let mut file_buf = [0u8; 260];
        let filter = b"Markdown Files (*.md;*.markdown;*.txt)\0*.md;*.markdown;*.txt\0All Files (*.*)\0*.*\0\0";
        let mut ofn: OPENFILENAMEA = unsafe { core::mem::zeroed() };
        ofn.lStructSize = core::mem::size_of::<OPENFILENAMEA>() as DWORD;
        ofn.hwndOwner = self.hwnd_main;
        ofn.lpstrFilter = filter.as_ptr();
        ofn.lpstrFile = file_buf.as_mut_ptr();
        ofn.nMaxFile = file_buf.len() as DWORD;
        ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY;
        if unsafe { GetOpenFileNameA(&mut ofn) } != 0 {
            let len = file_buf.iter().position(|&b| b == 0).unwrap_or(0);
            let path = cp1252_to_utf8(&file_buf[..len]);
            if !self.open_path(&path) {
                let msg = cstr("Failed to open selected file.");
                let cap = cstr("md98");
                unsafe {
                    MessageBoxA(self.hwnd_main, msg.as_ptr(), cap.as_ptr(), MB_OK | MB_ICONERROR);
                }
            }
        }
    }

    fn save_as_dialog(&mut self) {
        let mut file_buf = [0u8; 260];
        if let Some(cur) = &self.current_file {
            let bytes = utf8_to_cp1252(cur);
            let n = bytes.len().min(259);
            file_buf[..n].copy_from_slice(&bytes[..n]);
        }
        let filter = b"Markdown Files (*.md)\0*.md\0All Files (*.*)\0*.*\0\0";
        let def_ext = b"md\0";
        let mut ofn: OPENFILENAMEA = unsafe { core::mem::zeroed() };
        ofn.lStructSize = core::mem::size_of::<OPENFILENAMEA>() as DWORD;
        ofn.hwndOwner = self.hwnd_main;
        ofn.lpstrFilter = filter.as_ptr();
        ofn.lpstrFile = file_buf.as_mut_ptr();
        ofn.nMaxFile = file_buf.len() as DWORD;
        ofn.lpstrDefExt = def_ext.as_ptr();
        ofn.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST;
        if unsafe { GetSaveFileNameA(&mut ofn) } != 0 {
            let len = file_buf.iter().position(|&b| b == 0).unwrap_or(0);
            let path = cp1252_to_utf8(&file_buf[..len]);
            let _ = self.save_to_path(&path);
        }
    }

    fn run_stress_edits(&mut self, count: u32) {
        let base = self.utf8_buf.clone();
        for i in 0..count {
            let extra = format!("\n\nEdit #{}: **bold** *italic* `code_{}`\n", i + 1, i);
            let mut combined = String::with_capacity(base.len() + extra.len());
            combined.push_str(&base);
            combined.push_str(&extra);
            let crlf = normalize_to_crlf(&combined);
            let mut ansi = utf8_to_cp1252(&crlf);
            ansi.push(0);
            self.suppress_change = true;
            unsafe {
                SendMessageA(self.hwnd_edit, WM_SETTEXT, 0, ansi.as_ptr() as LPARAM);
            }
            self.suppress_change = false;
            self.relayout_preview_from_editor();
            unsafe {
                UpdateWindow(self.hwnd_preview);
            }
            self.stress_completed += 1;
        }
        // Restore original content cleanly
        self.load_markdown_text(&base, self.current_file.clone());
        unsafe {
            UpdateWindow(self.hwnd_preview);
        }
    }

    fn dump_metrics_file(&self) {
        if !self.cli.dump_metrics {
            return;
        }
        let mut ms: MEMORYSTATUS = unsafe { core::mem::zeroed() };
        ms.dwLength = core::mem::size_of::<MEMORYSTATUS>() as DWORD;
        unsafe {
            GlobalMemoryStatus(&mut ms);
        }
        let (cur_sel_min, cur_first_line) = unsafe {
            let mut cr = CHARRANGE::default();
            SendMessageA(self.hwnd_edit, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
            let fl = SendMessageA(self.hwnd_edit, EM_GETFIRSTVISIBLELINE, 0, 0) as i32;
            (cr.cpMin, fl)
        };
        let mode_str = match self.view_mode {
            ViewMode::Split => "split",
            ViewMode::EditorOnly => "editor",
            ViewMode::PreviewOnly => "preview",
        };
        let report = format!(
            "source_bytes={}\nblocks={}\ndraw_ops={}\ncontent_height={}\nlayout_ms={}\ndark_mode={}\nview_mode={}\nsync_scroll={}\nwrap_width={}\nlive_wrap={}\nreflow_on_save={}\nsplit_pos={}\nreplace_count={}\nedit_sel_min={}\nedit_first_line={}\npreview_scroll_y={}\ngdi_live_initial={}\ngdi_live_final={}\ngdi_delta={}\ngdi_created={}\ngdi_deleted={}\ngdi_live_dcs={}\npaint_count={}\nbackbuffer_reallocs={}\nstress_edits={}\nsave_test_bytes={}\nmem_load={}\ntotal_phys={}\navail_phys={}\n",
            self.utf8_buf.len(),
            self.layout.blocks.len(),
            self.layout.ops.len(),
            self.layout.content_height,
            self.last_layout_ms,
            if self.dark_mode { 1 } else { 0 },
            mode_str,
            if self.sync_scroll { 1 } else { 0 },
            self.wrap_width,
            if self.live_wrap { 1 } else { 0 },
            if self.reflow_on_save { 1 } else { 0 },
            self.split_pos,
            self.replace_count,
            cur_sel_min,
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
        let _ = write_file_bytes("C:\\MD98LOG.TXT", report.as_bytes());
    }
}

unsafe fn get_state(hwnd: HWND) -> *mut AppState {
    GetWindowLongA(hwnd, GWL_USERDATA) as *mut AppState
}

fn build_menu() -> HMENU {
    unsafe {
        let menu = CreateMenu();

        let file = CreatePopupMenu();
        AppendMenuA(file, MF_STRING, IDM_FILE_NEW, b"&New\tCtrl+N\0".as_ptr());
        AppendMenuA(file, MF_STRING, IDM_FILE_OPEN, b"&Open...\tCtrl+O\0".as_ptr());
        AppendMenuA(file, MF_STRING, IDM_FILE_SAVE, b"&Save\tCtrl+S\0".as_ptr());
        AppendMenuA(file, MF_STRING, IDM_FILE_SAVEAS, b"Save &As...\0".as_ptr());
        AppendMenuA(file, MF_SEPARATOR, 0, ptr::null());
        AppendMenuA(file, MF_STRING, IDM_FILE_EXIT, b"E&xit\tCtrl+Q\0".as_ptr());
        AppendMenuA(menu, MF_POPUP, file as usize, b"&File\0".as_ptr());

        let edit = CreatePopupMenu();
        AppendMenuA(edit, MF_STRING, IDM_EDIT_UNDO, b"&Undo\tCtrl+Z\0".as_ptr());
        AppendMenuA(edit, MF_SEPARATOR, 0, ptr::null());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_CUT, b"Cu&t\tCtrl+X\0".as_ptr());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_COPY, b"&Copy\tCtrl+C\0".as_ptr());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_PASTE, b"&Paste\tCtrl+V\0".as_ptr());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_SELECTALL, b"Select &All\tCtrl+A\0".as_ptr());
        AppendMenuA(edit, MF_SEPARATOR, 0, ptr::null());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_FIND, b"&Find...\tCtrl+F\0".as_ptr());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_FINDNEXT, b"Find &Next\tF3\0".as_ptr());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_REPLACE, b"&Replace...\tCtrl+H\0".as_ptr());
        AppendMenuA(edit, MF_SEPARATOR, 0, ptr::null());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_REFLOW, b"Reflo&w Text\tCtrl+R\0".as_ptr());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_LIVEWRAP, b"&Live Auto-Wrap\0".as_ptr());
        AppendMenuA(edit, MF_STRING, IDM_EDIT_REFLOW_ONSAVE, b"Reflow on Sa&ve\0".as_ptr());
        let wrap_sub = CreatePopupMenu();
        AppendMenuA(wrap_sub, MF_STRING, IDM_EDIT_WRAP_72, b"&72 Characters\0".as_ptr());
        AppendMenuA(wrap_sub, MF_STRING | MF_CHECKED, IDM_EDIT_WRAP_80, b"&80 Characters\0".as_ptr());
        AppendMenuA(wrap_sub, MF_STRING, IDM_EDIT_WRAP_100, b"&100 Characters\0".as_ptr());
        AppendMenuA(edit, MF_POPUP, wrap_sub as usize, b"Wrap &Width\0".as_ptr());
        AppendMenuA(menu, MF_POPUP, edit as usize, b"&Edit\0".as_ptr());

        let view = CreatePopupMenu();
        AppendMenuA(view, MF_STRING | MF_CHECKED, IDM_VIEW_SPLIT, b"&Split View\0".as_ptr());
        AppendMenuA(view, MF_STRING, IDM_VIEW_EDITOR, b"&Editor Only\0".as_ptr());
        AppendMenuA(view, MF_STRING, IDM_VIEW_PREVIEW, b"&Preview Only\0".as_ptr());
        AppendMenuA(view, MF_STRING, IDM_VIEW_TOGGLE_F11, b"&Toggle Preview/Editor\tF11\0".as_ptr());
        AppendMenuA(view, MF_SEPARATOR, 0, ptr::null());
        AppendMenuA(view, MF_STRING | MF_CHECKED, IDM_VIEW_SYNCSCROLL, b"Sync &Scroll\0".as_ptr());
        AppendMenuA(view, MF_STRING, IDM_VIEW_DARKMODE, b"&Dark Mode\0".as_ptr());
        AppendMenuA(menu, MF_POPUP, view as usize, b"&View\0".as_ptr());

        let help = CreatePopupMenu();
        AppendMenuA(help, MF_STRING, IDM_HELP_ABOUT, b"&About md98...\0".as_ptr());
        AppendMenuA(menu, MF_POPUP, help as usize, b"&Help\0".as_ptr());

        menu
    }
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
        WM_ERASEBKGND => 1, // Avoid flicker; WM_PAINT double-buffers entire viewport
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
            let code = loword(wparam);
            let max_y = (st.layout.content_height - st.preview_h).max(0);
            let new_y = match code {
                SB_LINEUP => st.scroll_y - 24,
                SB_LINEDOWN => st.scroll_y + 24,
                SB_PAGEUP => st.scroll_y - (st.preview_h - 32).max(24),
                SB_PAGEDOWN => st.scroll_y + (st.preview_h - 32).max(24),
                SB_TOP => 0,
                SB_BOTTOM => max_y,
                SB_THUMBTRACK | SB_THUMBPOSITION => {
                    let mut si: SCROLLINFO = core::mem::zeroed();
                    si.cbSize = core::mem::size_of::<SCROLLINFO>() as UINT;
                    si.fMask = SIF_ALL;
                    GetScrollInfo(hwnd, SB_VERT, &mut si);
                    si.nTrackPos
                }
                _ => st.scroll_y,
            };
            st.scroll_y = new_y.clamp(0, max_y);
            st.update_preview_scrollbars();
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        WM_HSCROLL => {
            let code = loword(wparam);
            let max_x = (st.layout.content_width - st.preview_w).max(0);
            let new_x = match code {
                SB_LINEUP => st.scroll_x - 24,
                SB_LINEDOWN => st.scroll_x + 24,
                SB_PAGEUP => st.scroll_x - (st.preview_w - 32).max(24),
                SB_PAGEDOWN => st.scroll_x + (st.preview_w - 32).max(24),
                SB_THUMBTRACK | SB_THUMBPOSITION => {
                    let mut si: SCROLLINFO = core::mem::zeroed();
                    si.cbSize = core::mem::size_of::<SCROLLINFO>() as UINT;
                    si.fMask = SIF_ALL;
                    GetScrollInfo(hwnd, SB_HORZ, &mut si);
                    si.nTrackPos
                }
                _ => st.scroll_x,
            };
            st.scroll_x = new_x.clamp(0, max_x);
            st.update_preview_scrollbars();
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        WM_MOUSEWHEEL => {
            let delta = hiword(wparam) as i16 as i32;
            let max_y = (st.layout.content_height - st.preview_h).max(0);
            let step = (delta / 120) * 48;
            st.scroll_y = (st.scroll_y - step).clamp(0, max_y);
            st.update_preview_scrollbars();
            InvalidateRect(hwnd, ptr::null(), 0);
            0
        }
        WM_LBUTTONDOWN => {
            SetFocus(hwnd);
            0
        }
        WM_KEYDOWN => {
            let max_y = (st.layout.content_height - st.preview_h).max(0);
            let delta = match wparam {
                VK_UP => -24,
                VK_DOWN => 24,
                VK_PRIOR => -(st.preview_h - 32).max(24),
                VK_NEXT => (st.preview_h - 32).max(24),
                _ => 0,
            };
            if delta != 0 {
                st.scroll_y = (st.scroll_y + delta).clamp(0, max_y);
                st.update_preview_scrollbars();
                InvalidateRect(hwnd, ptr::null(), 0);
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
            if st.view_mode == ViewMode::Split {
                let brush = if st.dark_mode {
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
            if st.view_mode == ViewMode::Split
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
            if st.view_mode == ViewMode::Split
                && x >= st.splitter_rect.left - 2
                && x <= st.splitter_rect.right + 2
            {
                st.dragging_splitter = true;
                SetCapture(hwnd);
                SetCursor(st.hcursor_sizewe);
                return 0;
            }
            0
        }
        WM_MOUSEMOVE => {
            if st.dragging_splitter {
                let x = get_x_lparam(lparam);
                st.split_pos = x;
                st.layout_children();
                InvalidateRect(hwnd, ptr::null(), 0);
                return 0;
            }
            0
        }
        WM_LBUTTONUP => {
            if st.dragging_splitter {
                st.dragging_splitter = false;
                ReleaseCapture();
                st.save_prefs();
            }
            0
        }
        WM_COMMAND => {
            let id = loword(wparam) as usize;
            let code = hiword(wparam);

            if id == ID_EDIT {
                if code == EN_CHANGE && !st.suppress_change {
                    if SendMessageA(st.hwnd_edit, EM_GETMODIFY, 0, 0) != 0 {
                        st.maybe_live_wrap_current_line();
                        st.modified = true;
                        st.update_title();
                        SetTimer(hwnd, TIMER_DEBOUNCE, DEBOUNCE_MS, ptr::null_mut());
                    }
                    return 0;
                } else if code == EN_VSCROLL {
                    st.sync_preview_scroll_to_editor();
                    return 0;
                }
            }

            match id {
                IDM_FILE_NEW => {
                    st.load_markdown_text("", None);
                }
                IDM_FILE_OPEN => {
                    st.open_dialog();
                }
                IDM_FILE_SAVE => {
                    if let Some(path) = st.current_file.clone() {
                        let _ = st.save_to_path(&path);
                    } else {
                        st.save_as_dialog();
                    }
                }
                IDM_FILE_SAVEAS => {
                    st.save_as_dialog();
                }
                IDM_FILE_EXIT => {
                    PostMessageA(hwnd, WM_CLOSE, 0, 0);
                }
                IDM_EDIT_UNDO => {
                    SendMessageA(st.hwnd_edit, EM_UNDO, 0, 0);
                }
                IDM_EDIT_CUT => {
                    SendMessageA(st.hwnd_edit, WM_CUT, 0, 0);
                }
                IDM_EDIT_COPY => {
                    SendMessageA(st.hwnd_edit, WM_COPY, 0, 0);
                }
                IDM_EDIT_PASTE => {
                    SendMessageA(st.hwnd_edit, WM_PASTE, 0, 0);
                }
                IDM_EDIT_SELECTALL => {
                    SendMessageA(st.hwnd_edit, EM_SETSEL, 0, -1);
                }
                IDM_EDIT_FIND => {
                    st.open_find_dialog(false);
                }
                IDM_EDIT_FINDNEXT => {
                    let _ = st.do_find_next(true);
                }
                IDM_EDIT_REPLACE => {
                    st.open_find_dialog(true);
                }
                IDM_EDIT_REFLOW => {
                    st.reflow_editor_text();
                }
                IDM_EDIT_LIVEWRAP => {
                    st.toggle_live_wrap();
                }
                IDM_EDIT_REFLOW_ONSAVE => {
                    st.toggle_reflow_on_save();
                }
                IDM_EDIT_WRAP_72 => {
                    st.set_wrap_width(72);
                }
                IDM_EDIT_WRAP_80 => {
                    st.set_wrap_width(80);
                }
                IDM_EDIT_WRAP_100 => {
                    st.set_wrap_width(100);
                }
                IDM_VIEW_SPLIT | IDM_VIEW_EDITOR | IDM_VIEW_PREVIEW => {
                    let target = match id {
                        IDM_VIEW_EDITOR => ViewMode::EditorOnly,
                        IDM_VIEW_PREVIEW => ViewMode::PreviewOnly,
                        _ => ViewMode::Split,
                    };
                    st.set_view_mode(target);
                }
                IDM_VIEW_TOGGLE_F11 => {
                    st.toggle_preview_editor_f11();
                }
                IDM_VIEW_DARKMODE => {
                    st.toggle_dark_mode();
                }
                IDM_VIEW_SYNCSCROLL => {
                    st.sync_scroll = !st.sync_scroll;
                    CheckMenuItem(
                        st.hmenu,
                        IDM_VIEW_SYNCSCROLL as UINT,
                        MF_BYCOMMAND | if st.sync_scroll { MF_CHECKED } else { MF_UNCHECKED },
                    );
                    st.save_prefs();
                }
                IDM_HELP_ABOUT => {
                    let msg = cstr("md98 - Native GFM Editor & Live Previewer for Windows 98 SE\r\nWritten in Rust (rust9x) for i586 Pentium+ (40 MB RAM).");
                    let cap = cstr("About md98");
                    MessageBoxA(hwnd, msg.as_ptr(), cap.as_ptr(), MB_OK | MB_ICONINFORMATION);
                }
                _ => return DefWindowProcA(hwnd, msg, wparam, lparam),
            }
            0
        }
        WM_TIMER => {
            if wparam == TIMER_DEBOUNCE {
                KillTimer(hwnd, TIMER_DEBOUNCE);
                st.relayout_preview_from_editor();
                st.sync_preview_scroll_to_editor();
                return 0;
            } else if wparam == TIMER_STRESS {
                KillTimer(hwnd, TIMER_STRESS);
                let n = st.cli.stress_edits;
                if n > 0 {
                    st.run_stress_edits(n);
                }
                if let Some(save_path) = st.cli.save_test_path.clone() {
                    let _ = st.save_to_path(&save_path);
                }
                return 0;
            } else if wparam == TIMER_HEADLESS_EXIT {
                KillTimer(hwnd, TIMER_HEADLESS_EXIT);
                st.dump_metrics_file();
                PostMessageA(hwnd, WM_CLOSE, 0, 0);
                return 0;
            }
            0
        }
        WM_SETFOCUS => {
            if st.view_mode != ViewMode::PreviewOnly {
                SetFocus(st.hwnd_edit);
            } else {
                SetFocus(st.hwnd_preview);
            }
            0
        }
        WM_CLOSE => {
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

pub fn run() {
    let cli = parse_cli();

    unsafe {
        let hinst = GetModuleHandleA(ptr::null());
        let icc = INITCOMMONCONTROLSEX {
            dwSize: core::mem::size_of::<INITCOMMONCONTROLSEX>() as DWORD,
            dwICC: ICC_BAR_CLASSES,
        };
        InitCommonControlsEx(&icc);

        // Load RichEdit 2.0 (stock on Windows 98 SE — zero 64 KB EDIT limit!)
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

        let hmenu = build_menu();
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
            ID_EDIT as HMENU,
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
            ID_PREVIEW as HMENU,
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
            ID_STATUS as HMENU,
            hinst,
            ptr::null_mut(),
        );

        let gdi = GdiCache::new();
        // Configure RichEdit 2.0: 4 MB limit (no 64 KB cap!), monospace font, EN_CHANGE + EN_VSCROLL
        SendMessageA(hwnd_edit, EM_EXLIMITTEXT, 0, (4 * 1024 * 1024) as LPARAM);
        SendMessageA(hwnd_edit, WM_SETFONT, gdi.editor_font as WPARAM, 1);
        SendMessageA(hwnd_edit, EM_SETEVENTMASK, 0, ENM_CHANGE | ENM_SCROLL);

        let msg_find_replace = RegisterWindowMessageA(b"commdlg_FindReplace\0".as_ptr());
        let prefs = load_prefs_from_registry();
        let start_dark = if cli.dark_mode { true } else { prefs.dark_mode };
        let start_wrap_width = cli.wrap_width.unwrap_or(prefs.wrap_width);
        let start_live_wrap = if cli.live_wrap { true } else { prefs.live_wrap };
        let start_reflow_on_save = if cli.reflow_on_save {
            true
        } else {
            prefs.reflow_on_save
        };
        let mut state = Box::new(AppState {
            hwnd_main,
            hwnd_edit,
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
            wrap_width: start_wrap_width,
            live_wrap: start_live_wrap,
            reflow_on_save: start_reflow_on_save,
            gdi,
            backbuf: ViewportBackBuffer::new(),
            layout: DocumentLayout::new(),
            raw_ansi_buf: Vec::with_capacity(8192),
            utf8_buf: String::with_capacity(8192),
            current_file: None,
            modified: false,
            suppress_change: false,
            dark_mode: start_dark,
            view_mode: prefs.view_mode,
            sync_scroll: prefs.sync_scroll,
            split_pos: prefs.split_pos,
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
        if state.view_mode == ViewMode::PreviewOnly {
            SetFocus(hwnd_preview);
        } else {
            SetFocus(hwnd_edit);
        }

        // Record baseline live GDI handles after initial layout & first paint
        state.gdi_live_initial = state.gdi.stats.live_objects();

        if state.cli.stress_edits > 0 || state.cli.save_test_path.is_some() {
            SetTimer(hwnd_main, TIMER_STRESS, 300, ptr::null_mut());
        }
        if let Some(ms) = state.cli.headless_exit_ms {
            SetTimer(hwnd_main, TIMER_HEADLESS_EXIT, ms.max(500), ptr::null_mut());
        }

        let mut msg: MSG = core::mem::zeroed();
        while GetMessageA(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            if !state.hwnd_find.is_null() && IsDialogMessageA(state.hwnd_find, &msg) != 0 {
                continue;
            }
            if msg.message == WM_KEYDOWN {
                if msg.wParam == VK_F11 {
                    PostMessageA(hwnd_main, WM_COMMAND, IDM_VIEW_TOGGLE_F11, 0);
                    continue;
                }
                if msg.wParam == VK_F3 {
                    PostMessageA(hwnd_main, WM_COMMAND, IDM_EDIT_FINDNEXT, 0);
                    continue;
                }
                // Simple keyboard shortcuts (Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Q, Ctrl+F, Ctrl+H, Ctrl+R)
                if (GetKeyState(VK_CONTROL) as u16 & 0x8000) != 0 {
                    match msg.wParam as u8 {
                        b'N' => {
                            PostMessageA(hwnd_main, WM_COMMAND, IDM_FILE_NEW, 0);
                            continue;
                        }
                        b'O' => {
                            PostMessageA(hwnd_main, WM_COMMAND, IDM_FILE_OPEN, 0);
                            continue;
                        }
                        b'S' => {
                            PostMessageA(hwnd_main, WM_COMMAND, IDM_FILE_SAVE, 0);
                            continue;
                        }
                        b'Q' => {
                            PostMessageA(hwnd_main, WM_COMMAND, IDM_FILE_EXIT, 0);
                            continue;
                        }
                        b'F' => {
                            PostMessageA(hwnd_main, WM_COMMAND, IDM_EDIT_FIND, 0);
                            continue;
                        }
                        b'H' => {
                            PostMessageA(hwnd_main, WM_COMMAND, IDM_EDIT_REPLACE, 0);
                            continue;
                        }
                        b'R' => {
                            PostMessageA(hwnd_main, WM_COMMAND, IDM_EDIT_REFLOW, 0);
                            continue;
                        }
                        _ => {}
                    }
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
