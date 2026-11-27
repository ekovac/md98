//! Safe Win32 ANSI (`*A`) wrappers for file I/O, dialogs, menus, timers, scrollbars,
//! status bar, and `RichEdit 2.0`.

#![cfg(windows)]

use crate::types::{MenuId, MessageIcon, SavePromptChoice, ScrollBar, TimerId};
use crate::win32_ffi::*;
use core::ptr;
use md98_core::{cp1252_to_utf8, normalize_to_crlf, normalize_to_lf, utf8_to_cp1252};

/// Convert UTF-8 string to NUL-terminated Windows-1252 (CP1252) bytes for Win32 `*A` APIs.
pub fn cstr(s: &str) -> Vec<u8> {
    let mut v = utf8_to_cp1252(s);
    v.push(0);
    v
}

/// Read the process command line as UTF-8 (transcoded from Win98 CP1252).
pub fn command_line_utf8() -> String {
    unsafe {
        let p = GetCommandLineA();
        if p.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        while *p.add(len) != 0 && len < 4096 {
            len += 1;
        }
        cp1252_to_utf8(core::slice::from_raw_parts(p, len))
    }
}

pub fn tick_count() -> u32 {
    unsafe { GetTickCount() }
}

pub fn memory_status() -> MEMORYSTATUS {
    let mut ms: MEMORYSTATUS = unsafe { core::mem::zeroed() };
    ms.dwLength = core::mem::size_of::<MEMORYSTATUS>() as DWORD;
    unsafe {
        GlobalMemoryStatus(&mut ms);
    }
    ms
}

pub fn ctrl_is_down() -> bool {
    unsafe { (GetKeyState(VK_CONTROL) as u16 & 0x8000) != 0 }
}

pub fn read_file_bytes(path: &str) -> Option<Vec<u8>> {
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

pub fn write_file_bytes(path: &str, data: &[u8]) -> bool {
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

pub fn open_file_dialog(owner: HWND) -> Option<String> {
    let mut file_buf = [0u8; 260];
    let filter = b"Markdown Files (*.md;*.markdown;*.txt)\0*.md;*.markdown;*.txt\0All Files (*.*)\0*.*\0\0";
    let mut ofn: OPENFILENAMEA = unsafe { core::mem::zeroed() };
    ofn.lStructSize = core::mem::size_of::<OPENFILENAMEA>() as DWORD;
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file_buf.as_mut_ptr();
    ofn.nMaxFile = file_buf.len() as DWORD;
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY;
    if unsafe { GetOpenFileNameA(&mut ofn) } != 0 {
        let len = file_buf.iter().position(|&b| b == 0).unwrap_or(0);
        Some(cp1252_to_utf8(&file_buf[..len]))
    } else {
        None
    }
}

pub fn save_file_dialog(owner: HWND, initial_path: Option<&str>) -> Option<String> {
    let mut file_buf = [0u8; 260];
    if let Some(cur) = initial_path {
        let bytes = utf8_to_cp1252(cur);
        let n = bytes.len().min(259);
        file_buf[..n].copy_from_slice(&bytes[..n]);
    }
    let filter = b"Markdown Files (*.md)\0*.md\0All Files (*.*)\0*.*\0\0";
    let def_ext = b"md\0";
    let mut ofn: OPENFILENAMEA = unsafe { core::mem::zeroed() };
    ofn.lStructSize = core::mem::size_of::<OPENFILENAMEA>() as DWORD;
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file_buf.as_mut_ptr();
    ofn.nMaxFile = file_buf.len() as DWORD;
    ofn.lpstrDefExt = def_ext.as_ptr();
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST;
    if unsafe { GetSaveFileNameA(&mut ofn) } != 0 {
        let len = file_buf.iter().position(|&b| b == 0).unwrap_or(0);
        Some(cp1252_to_utf8(&file_buf[..len]))
    } else {
        None
    }
}

pub fn message_box_ok(owner: HWND, title: &str, text: &str, icon: MessageIcon) {
    let flags = MB_OK
        | match icon {
            MessageIcon::Info => MB_ICONINFORMATION,
            MessageIcon::Error => MB_ICONERROR,
        };
    let msg = cstr(text);
    let cap = cstr(title);
    unsafe {
        MessageBoxA(owner, msg.as_ptr(), cap.as_ptr(), flags);
    }
}

pub fn confirm_save_changes(owner: HWND, display_name: &str) -> SavePromptChoice {
    let msg = cstr(&format!("Save changes to {}?", display_name));
    let cap = cstr("md98");
    let choice = unsafe {
        MessageBoxA(
            owner,
            msg.as_ptr(),
            cap.as_ptr(),
            MB_YESNOCANCEL | MB_ICONQUESTION,
        )
    };
    match choice {
        IDYES => SavePromptChoice::Save,
        IDNO => SavePromptChoice::Discard,
        _ => SavePromptChoice::Cancel,
    }
}

pub fn set_window_text(hwnd: HWND, text: &str) {
    let s = cstr(text);
    unsafe {
        SetWindowTextA(hwnd, s.as_ptr());
    }
}

pub fn invalidate(hwnd: HWND, erase: bool) {
    unsafe {
        InvalidateRect(hwnd, ptr::null(), i32::from(erase));
    }
}

pub fn set_focus(hwnd: HWND) {
    unsafe {
        SetFocus(hwnd);
    }
}

pub fn post_command(hwnd: HWND, id: MenuId) {
    unsafe {
        PostMessageA(hwnd, WM_COMMAND, id as usize, 0);
    }
}

pub fn post_close(hwnd: HWND) {
    unsafe {
        PostMessageA(hwnd, WM_CLOSE, 0, 0);
    }
}

pub fn set_timer(hwnd: HWND, id: TimerId, ms: u32) {
    unsafe {
        SetTimer(hwnd, id as usize, ms, ptr::null_mut());
    }
}

pub fn kill_timer(hwnd: HWND, id: TimerId) {
    unsafe {
        KillTimer(hwnd, id as usize);
    }
}

pub fn check_menu_item(hmenu: HMENU, id: MenuId, checked: bool) {
    let flags = MF_BYCOMMAND | if checked { MF_CHECKED } else { MF_UNCHECKED };
    unsafe {
        CheckMenuItem(hmenu, id as UINT, flags);
    }
}

pub fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rc = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut rc);
    }
    (rc.right - rc.left, rc.bottom - rc.top)
}

pub fn set_scroll_info(hwnd: HWND, bar: ScrollBar, max: i32, page: i32, pos: i32) {
    let si = SCROLLINFO {
        cbSize: core::mem::size_of::<SCROLLINFO>() as UINT,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: max.max(page),
        nPage: page.max(1) as UINT,
        nPos: pos,
        nTrackPos: 0,
    };
    unsafe {
        SetScrollInfo(hwnd, bar as i32, &si, 1);
    }
}

pub fn get_scroll_track_pos(hwnd: HWND, bar: ScrollBar) -> i32 {
    let mut si: SCROLLINFO = unsafe { core::mem::zeroed() };
    si.cbSize = core::mem::size_of::<SCROLLINFO>() as UINT;
    si.fMask = SIF_ALL;
    unsafe {
        GetScrollInfo(hwnd, bar as i32, &mut si);
    }
    si.nTrackPos
}

pub fn set_status_parts(hwnd_status: HWND, parts: &[i32]) {
    if hwnd_status.is_null() {
        return;
    }
    unsafe {
        SendMessageA(
            hwnd_status,
            SB_SETPARTS,
            parts.len(),
            parts.as_ptr() as LPARAM,
        );
    }
}

pub fn set_status_part(hwnd_status: HWND, part: usize, text: &str) {
    if hwnd_status.is_null() {
        return;
    }
    let s = cstr(text);
    unsafe {
        SendMessageA(hwnd_status, SB_SETTEXTA, part, s.as_ptr() as LPARAM);
    }
}

fn append_menu_item(popup: HMENU, id: MenuId, label: &[u8], checked: bool) {
    let flags = MF_STRING | if checked { MF_CHECKED } else { 0 };
    unsafe {
        AppendMenuA(popup, flags, id as usize, label.as_ptr());
    }
}

fn append_menu_sep(popup: HMENU) {
    unsafe {
        AppendMenuA(popup, MF_SEPARATOR, 0, ptr::null());
    }
}

pub fn build_main_menu() -> HMENU {
    unsafe {
        let menu = CreateMenu();

        let file = CreatePopupMenu();
        append_menu_item(file, MenuId::FileNew, b"&New\tCtrl+N\0", false);
        append_menu_item(file, MenuId::FileOpen, b"&Open...\tCtrl+O\0", false);
        append_menu_item(file, MenuId::FileSave, b"&Save\tCtrl+S\0", false);
        append_menu_item(file, MenuId::FileSaveAs, b"Save &As...\0", false);
        append_menu_sep(file);
        append_menu_item(file, MenuId::FileExit, b"E&xit\tCtrl+Q\0", false);
        AppendMenuA(menu, MF_POPUP, file as usize, b"&File\0".as_ptr());

        let edit = CreatePopupMenu();
        append_menu_item(edit, MenuId::EditUndo, b"&Undo\tCtrl+Z\0", false);
        append_menu_sep(edit);
        append_menu_item(edit, MenuId::EditCut, b"Cu&t\tCtrl+X\0", false);
        append_menu_item(edit, MenuId::EditCopy, b"&Copy\tCtrl+C\0", false);
        append_menu_item(edit, MenuId::EditPaste, b"&Paste\tCtrl+V\0", false);
        append_menu_item(edit, MenuId::EditSelectAll, b"Select &All\tCtrl+A\0", false);
        append_menu_sep(edit);
        append_menu_item(edit, MenuId::EditFind, b"&Find...\tCtrl+F\0", false);
        append_menu_item(edit, MenuId::EditFindNext, b"Find &Next\tF3\0", false);
        append_menu_item(edit, MenuId::EditReplace, b"&Replace...\tCtrl+H\0", false);
        append_menu_sep(edit);
        append_menu_item(edit, MenuId::EditReflow, b"Reflo&w Text\tCtrl+R\0", false);
        append_menu_item(edit, MenuId::EditLiveWrap, b"&Live Auto-Wrap\0", false);
        append_menu_item(edit, MenuId::EditReflowOnSave, b"Reflow on Sa&ve\0", false);
        let wrap_sub = CreatePopupMenu();
        append_menu_item(wrap_sub, MenuId::EditWrap72, b"&72 Characters\0", false);
        append_menu_item(wrap_sub, MenuId::EditWrap80, b"&80 Characters\0", true);
        append_menu_item(wrap_sub, MenuId::EditWrap100, b"&100 Characters\0", false);
        AppendMenuA(edit, MF_POPUP, wrap_sub as usize, b"Wrap &Width\0".as_ptr());
        AppendMenuA(menu, MF_POPUP, edit as usize, b"&Edit\0".as_ptr());

        let view = CreatePopupMenu();
        append_menu_item(view, MenuId::ViewSplit, b"&Split View\0", true);
        append_menu_item(view, MenuId::ViewEditor, b"&Editor Only\0", false);
        append_menu_item(view, MenuId::ViewPreview, b"&Preview Only\0", false);
        append_menu_item(view, MenuId::ViewToggleF11, b"&Toggle Preview/Editor\tF11\0", false);
        append_menu_sep(view);
        append_menu_item(view, MenuId::ViewSyncScroll, b"Sync &Scroll\0", true);
        append_menu_item(view, MenuId::ViewDarkMode, b"&Dark Mode\0", false);
        AppendMenuA(menu, MF_POPUP, view as usize, b"&View\0".as_ptr());

        let help = CreatePopupMenu();
        append_menu_item(help, MenuId::HelpAbout, b"&About md98...\0", false);
        AppendMenuA(menu, MF_POPUP, help as usize, b"&Help\0".as_ptr());

        menu
    }
}

/// Safe wrapper around the `RichEdit 2.0` child window (`RICHED20.DLL`).
#[derive(Clone, Copy)]
pub struct RichEdit(pub HWND);

impl RichEdit {
    pub fn configure(self, font: HFONT) {
        unsafe {
            // 4 MB limit (no 64 KB Win98 EDIT cap!), monospace font, change + scroll + selection events
            SendMessageA(self.0, EM_EXLIMITTEXT, 0, (4 * 1024 * 1024) as LPARAM);
            SendMessageA(self.0, WM_SETFONT, font as WPARAM, 1);
            SendMessageA(
                self.0,
                EM_SETEVENTMASK,
                0,
                ENM_CHANGE | ENM_SCROLL | ENM_SELCHANGE,
            );
        }
    }

    pub fn get_selection(self) -> CHARRANGE {
        let mut cr = CHARRANGE::default();
        unsafe {
            SendMessageA(self.0, EM_EXGETSEL, 0, &mut cr as *mut _ as LPARAM);
        }
        cr
    }

    pub fn set_selection(self, cr: CHARRANGE) {
        unsafe {
            SendMessageA(self.0, EM_EXSETSEL, 0, &cr as *const _ as LPARAM);
        }
    }

    pub fn select_all(self) {
        unsafe {
            SendMessageA(self.0, EM_SETSEL, 0, -1);
        }
    }

    pub fn scroll_caret(self) {
        unsafe {
            SendMessageA(self.0, EM_SCROLLCARET, 0, 0);
        }
    }

    pub fn first_visible_line(self) -> i32 {
        unsafe { SendMessageA(self.0, EM_GETFIRSTVISIBLELINE, 0, 0) as i32 }
    }

    pub fn line_count(self) -> i32 {
        unsafe { SendMessageA(self.0, EM_GETLINECOUNT, 0, 0) as i32 }
    }

    pub fn scroll_to_line(self, target_first_line: i32) {
        let cur = self.first_visible_line();
        let delta = target_first_line - cur;
        if delta != 0 {
            unsafe {
                SendMessageA(self.0, EM_LINESCROLL, 0, delta as LPARAM);
            }
        }
    }

    /// Compute 1-based `(line, col)` directly from RichEdit in O(1).
    pub fn cursor_line_col(self) -> (i32, i32) {
        let cr = self.get_selection();
        unsafe {
            let line = SendMessageA(self.0, EM_EXLINEFROMCHAR, 0, cr.cpMin as LPARAM) as i32;
            let line_start = SendMessageA(self.0, EM_LINEINDEX, line as WPARAM, 0) as i32;
            let col = (cr.cpMin - line_start).max(0) + 1;
            (line + 1, col)
        }
    }

    pub fn is_modified(self) -> bool {
        unsafe { SendMessageA(self.0, EM_GETMODIFY, 0, 0) != 0 }
    }

    pub fn set_modified(self, modified: bool) {
        unsafe {
            SendMessageA(self.0, EM_SETMODIFY, usize::from(modified), 0);
        }
    }

    pub fn undo(self) {
        unsafe {
            SendMessageA(self.0, EM_UNDO, 0, 0);
        }
    }

    pub fn cut(self) {
        unsafe {
            SendMessageA(self.0, WM_CUT, 0, 0);
        }
    }

    pub fn copy(self) {
        unsafe {
            SendMessageA(self.0, WM_COPY, 0, 0);
        }
    }

    pub fn paste(self) {
        unsafe {
            SendMessageA(self.0, WM_PASTE, 0, 0);
        }
    }

    pub fn get_selected_cp1252(self, cr: CHARRANGE) -> Vec<u8> {
        if cr.cpMax <= cr.cpMin {
            return Vec::new();
        }
        let sel_len = (cr.cpMax - cr.cpMin) as usize;
        let mut buf = vec![0u8; sel_len + 4];
        let copied =
            unsafe { SendMessageA(self.0, EM_GETSELTEXT, 0, buf.as_mut_ptr() as LPARAM) } as usize;
        buf.truncate(copied.min(buf.len()));
        buf
    }

    pub fn set_text_lf(self, utf8_lf: &str) {
        let crlf = normalize_to_crlf(utf8_lf);
        let mut ansi = utf8_to_cp1252(&crlf);
        ansi.push(0);
        unsafe {
            SendMessageA(self.0, WM_SETTEXT, 0, ansi.as_ptr() as LPARAM);
        }
    }

    pub fn replace_selection_raw_cp1252(self, utf8_text: &str, undoable: bool) {
        let mut ansi = utf8_to_cp1252(utf8_text);
        ansi.push(0);
        unsafe {
            SendMessageA(
                self.0,
                EM_REPLACESEL,
                usize::from(undoable),
                ansi.as_ptr() as LPARAM,
            );
        }
    }

    pub fn replace_selection_lf(self, utf8_lf: &str, undoable: bool) {
        let crlf = normalize_to_crlf(utf8_lf);
        self.replace_selection_raw_cp1252(&crlf, undoable);
    }

    pub fn read_to_utf8_lf(self, raw_ansi_buf: &mut Vec<u8>) -> String {
        let len = unsafe { SendMessageA(self.0, WM_GETTEXTLENGTH, 0, 0) } as usize;
        raw_ansi_buf.clear();
        raw_ansi_buf.resize(len + 1, 0);
        let copied = unsafe {
            SendMessageA(
                self.0,
                WM_GETTEXT,
                raw_ansi_buf.len(),
                raw_ansi_buf.as_mut_ptr() as LPARAM,
            )
        } as usize;
        let actual = copied.min(len);
        let decoded = cp1252_to_utf8(&raw_ansi_buf[..actual]);
        normalize_to_lf(&decoded)
    }

    /// Apply editor background and foreground colors while preserving caret, scroll, and dirty state.
    pub fn apply_theme(self, bg: COLORREF, fg: COLORREF) {
        let was_modified = self.is_modified();
        let cr = self.get_selection();
        let first_line = self.first_visible_line();

        unsafe {
            // Suppress EN_CHANGE notifications while applying purely visual theme formatting
            SendMessageA(self.0, EM_SETEVENTMASK, 0, ENM_SCROLL | ENM_SELCHANGE);
            SendMessageA(self.0, EM_SETBKGNDCOLOR, 0, bg as LPARAM);

            let mut cf: CHARFORMATA = core::mem::zeroed();
            cf.cbSize = core::mem::size_of::<CHARFORMATA>() as UINT;
            cf.dwMask = CFM_COLOR;
            cf.crTextColor = fg;
            SendMessageA(self.0, EM_SETCHARFORMAT, SCF_ALL, &cf as *const _ as LPARAM);
            SendMessageA(self.0, EM_SETCHARFORMAT, SCF_DEFAULT, &cf as *const _ as LPARAM);
        }

        self.set_selection(cr);
        self.scroll_to_line(first_line);
        self.set_modified(was_modified);
        unsafe {
            SendMessageA(
                self.0,
                EM_SETEVENTMASK,
                0,
                ENM_CHANGE | ENM_SCROLL | ENM_SELCHANGE,
            );
        }
        invalidate(self.0, true);
    }
}
