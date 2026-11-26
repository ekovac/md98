//! Minimal Win32 ANSI (`*A`) FFI declarations restricted strictly to stock Windows 98 SE exports.
//!
//! Allowed DLLs: KERNEL32.DLL, USER32.DLL, GDI32.DLL, COMDLG32.DLL, COMCTL32.DLL, RICHED20.DLL.

#![cfg(windows)]
#![allow(dead_code, non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use core::ffi::c_void;

pub type HWND = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type HMODULE = *mut c_void;
pub type HDC = *mut c_void;
pub type HFONT = *mut c_void;
pub type HBRUSH = *mut c_void;
pub type HPEN = *mut c_void;
pub type HBITMAP = *mut c_void;
pub type HGDIOBJ = *mut c_void;
pub type HMENU = *mut c_void;
pub type HICON = *mut c_void;
pub type HCURSOR = *mut c_void;
pub type HANDLE = *mut c_void;
pub type UINT = u32;
pub type DWORD = u32;
pub type WORD = u16;
pub type BOOL = i32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type COLORREF = u32;
pub type ATOM = u16;

pub type WNDPROC = unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT;

// Window styles
pub const WS_OVERLAPPEDWINDOW: DWORD = 0x00CF_0000;
pub const WS_CHILD: DWORD = 0x4000_0000;
pub const WS_VISIBLE: DWORD = 0x1000_0000;
pub const WS_VSCROLL: DWORD = 0x0020_0000;
pub const WS_HSCROLL: DWORD = 0x0010_0000;
pub const WS_BORDER: DWORD = 0x0080_0000;
pub const WS_CLIPCHILDREN: DWORD = 0x0200_0000;
pub const WS_CLIPSIBLINGS: DWORD = 0x0400_0000;
pub const WS_EX_CLIENTEDGE: DWORD = 0x0000_0200;

// RichEdit / Edit styles
pub const ES_MULTILINE: DWORD = 0x0004;
pub const ES_AUTOVSCROLL: DWORD = 0x0040;
pub const ES_AUTOHSCROLL: DWORD = 0x0080;
pub const ES_NOHIDESEL: DWORD = 0x0100;
pub const ES_WANTRETURN: DWORD = 0x1000;

// StatusBar / Toolbar styles
pub const SBARS_SIZEGRIP: DWORD = 0x0100;
pub const CCS_BOTTOM: DWORD = 0x0003;
pub const CCS_TOP: DWORD = 0x0001;
pub const TBSTYLE_FLAT: DWORD = 0x0800;
pub const TBSTYLE_TOOLTIPS: DWORD = 0x0100;
pub const TBSTATE_ENABLED: u8 = 0x04;
pub const TBSTYLE_BUTTON: u8 = 0x00;
pub const TBSTYLE_SEP: u8 = 0x01;

// Window class styles
pub const CS_HREDRAW: UINT = 0x0002;
pub const CS_VREDRAW: UINT = 0x0001;

// Messages
pub const WM_CREATE: UINT = 0x0001;
pub const WM_DESTROY: UINT = 0x0002;
pub const WM_SIZE: UINT = 0x0005;
pub const WM_SETFOCUS: UINT = 0x0007;
pub const WM_SETTEXT: UINT = 0x000C;
pub const WM_GETTEXT: UINT = 0x000D;
pub const WM_GETTEXTLENGTH: UINT = 0x000E;
pub const WM_PAINT: UINT = 0x000F;
pub const WM_CLOSE: UINT = 0x0010;
pub const WM_QUIT: UINT = 0x0012;
pub const WM_ERASEBKGND: UINT = 0x0014;
pub const WM_SETCURSOR: UINT = 0x0020;
pub const WM_SETFONT: UINT = 0x0030;
pub const WM_NOTIFY: UINT = 0x004E;
pub const WM_KEYDOWN: UINT = 0x0100;
pub const WM_COMMAND: UINT = 0x0111;
pub const WM_TIMER: UINT = 0x0113;
pub const WM_HSCROLL: UINT = 0x0114;
pub const WM_VSCROLL: UINT = 0x0115;
pub const WM_MOUSEMOVE: UINT = 0x0200;
pub const WM_LBUTTONDOWN: UINT = 0x0201;
pub const WM_LBUTTONUP: UINT = 0x0202;
pub const WM_MOUSEWHEEL: UINT = 0x020A;
pub const WM_USER: UINT = 0x0400;

// Edit / RichEdit messages & notifications
pub const EM_GETFIRSTVISIBLELINE: UINT = 0x00CE;
pub const EM_GETLINECOUNT: UINT = 0x00BA;
pub const EM_LINEINDEX: UINT = 0x00BB;
pub const EM_UNDO: UINT = 0x00C7;
pub const WM_CUT: UINT = 0x0300;
pub const WM_COPY: UINT = 0x0301;
pub const WM_PASTE: UINT = 0x0302;
pub const EM_SETSEL: UINT = 0x00B1;
pub const EM_EXGETSEL: UINT = WM_USER + 52;
pub const EM_EXLIMITTEXT: UINT = WM_USER + 53;
pub const EM_EXLINEFROMCHAR: UINT = WM_USER + 54;
pub const EM_SETEVENTMASK: UINT = WM_USER + 69;
pub const ENM_CHANGE: LPARAM = 0x0000_0001;
pub const ENM_SCROLL: LPARAM = 0x0000_0004;
pub const EN_CHANGE: u16 = 0x0300;
pub const EN_VSCROLL: u16 = 0x0602;

// Status bar messages
pub const SB_SETPARTS: UINT = WM_USER + 4;
pub const SB_SETTEXTA: UINT = WM_USER + 1;

// Scrollbar constants
pub const SB_HORZ: i32 = 0;
pub const SB_VERT: i32 = 1;
pub const SB_LINEUP: u16 = 0;
pub const SB_LINEDOWN: u16 = 1;
pub const SB_PAGEUP: u16 = 2;
pub const SB_PAGEDOWN: u16 = 3;
pub const SB_THUMBTRACK: u16 = 5;
pub const SB_THUMBPOSITION: u16 = 4;
pub const SB_TOP: u16 = 6;
pub const SB_BOTTOM: u16 = 7;
pub const SIF_RANGE: UINT = 0x0001;
pub const SIF_PAGE: UINT = 0x0002;
pub const SIF_POS: UINT = 0x0004;
pub const SIF_TRACKPOS: UINT = 0x0010;
pub const SIF_ALL: UINT = SIF_RANGE | SIF_PAGE | SIF_POS | SIF_TRACKPOS;

// Cursors & Icons
pub const IDC_ARROW: *const u8 = 32512 as *const u8;
pub const IDC_SIZEWE: *const u8 = 32644 as *const u8;
pub const IDI_APPLICATION: *const u8 = 32512 as *const u8;

// ShowWindow / SetWindowPos / Menus
pub const SW_SHOW: i32 = 5;
pub const SW_MAXIMIZE: i32 = 3;
pub const CW_USEDEFAULT: i32 = 0x8000_0000u32 as i32;
pub const GWL_USERDATA: i32 = -21;
pub const COLOR_BTNFACE: i32 = 15;
pub const COLOR_WINDOW: i32 = 5;
pub const MF_STRING: UINT = 0x0000;
pub const MF_POPUP: UINT = 0x0010;
pub const MF_SEPARATOR: UINT = 0x0800;
pub const MF_CHECKED: UINT = 0x0008;
pub const MF_UNCHECKED: UINT = 0x0000;
pub const MF_BYCOMMAND: UINT = 0x0000;
pub const MB_OK: UINT = 0x0000;
pub const MB_ICONINFORMATION: UINT = 0x0040;
pub const MB_ICONERROR: UINT = 0x0010;

// GDI constants
pub const TRANSPARENT: i32 = 1;
pub const SRCCOPY: DWORD = 0x00CC_0020;
pub const PS_SOLID: i32 = 0;
pub const NULL_BRUSH: i32 = 5;
pub const FW_NORMAL: i32 = 400;
pub const FW_BOLD: i32 = 700;
pub const DEFAULT_CHARSET: DWORD = 1;
pub const ANSI_CHARSET: DWORD = 0;
pub const OUT_DEFAULT_PRECIS: DWORD = 0;
pub const CLIP_DEFAULT_PRECIS: DWORD = 0;
pub const DEFAULT_QUALITY: DWORD = 0;
pub const DEFAULT_PITCH: DWORD = 0;
pub const FIXED_PITCH: DWORD = 1;
pub const FF_DONTCARE: DWORD = 0;
pub const FF_MODERN: DWORD = 48;
pub const FF_SWISS: DWORD = 32;
pub const DFC_BUTTON: UINT = 4;
pub const DFCS_BUTTONCHECK: UINT = 0x0000;
pub const DFCS_CHECKED: UINT = 0x0400;

// File / Common Dialog constants
pub const INVALID_HANDLE_VALUE: HANDLE = (-1isize) as HANDLE;
pub const GENERIC_READ: DWORD = 0x8000_0000;
pub const GENERIC_WRITE: DWORD = 0x4000_0000;
pub const FILE_SHARE_READ: DWORD = 0x0000_0001;
pub const OPEN_EXISTING: DWORD = 3;
pub const CREATE_ALWAYS: DWORD = 2;
pub const FILE_ATTRIBUTE_NORMAL: DWORD = 0x0000_0080;
pub const OFN_FILEMUSTEXIST: DWORD = 0x0000_1000;
pub const OFN_PATHMUSTEXIST: DWORD = 0x0000_0800;
pub const OFN_OVERWRITEPROMPT: DWORD = 0x0000_0002;
pub const OFN_HIDEREADONLY: DWORD = 0x0000_0004;
pub const ICC_BAR_CLASSES: DWORD = 0x0000_0004;

// Virtual keys
pub const VK_CONTROL: i32 = 0x11;
pub const VK_PRIOR: usize = 0x21;
pub const VK_NEXT: usize = 0x22;
pub const VK_UP: usize = 0x26;
pub const VK_DOWN: usize = 0x28;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}

#[repr(C)]
pub struct WNDCLASSA {
    pub style: UINT,
    pub lpfnWndProc: Option<WNDPROC>,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: *const u8,
    pub lpszClassName: *const u8,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: UINT,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: DWORD,
    pub pt: POINT,
}

#[repr(C)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: BOOL,
    pub rcPaint: RECT,
    pub fRestore: BOOL,
    pub fIncUpdate: BOOL,
    pub rgbReserved: [u8; 32],
}

#[repr(C)]
pub struct SCROLLINFO {
    pub cbSize: UINT,
    pub fMask: UINT,
    pub nMin: i32,
    pub nMax: i32,
    pub nPage: UINT,
    pub nPos: i32,
    pub nTrackPos: i32,
}

#[repr(C)]
pub struct LOGFONTA {
    pub lfHeight: i32,
    pub lfWidth: i32,
    pub lfEscapement: i32,
    pub lfOrientation: i32,
    pub lfWeight: i32,
    pub lfItalic: u8,
    pub lfUnderline: u8,
    pub lfStrikeOut: u8,
    pub lfCharSet: u8,
    pub lfOutPrecision: u8,
    pub lfClipPrecision: u8,
    pub lfQuality: u8,
    pub lfPitchAndFamily: u8,
    pub lfFaceName: [u8; 32],
}

#[repr(C)]
pub struct CHARRANGE {
    pub cpMin: i32,
    pub cpMax: i32,
}

#[repr(C)]
pub struct INITCOMMONCONTROLSEX {
    pub dwSize: DWORD,
    pub dwICC: DWORD,
}

/// Win98 SE-compatible `OPENFILENAMEA` (76 bytes — pre-Win2000 size without `pvReserved`/`dwReserved`/`FlagsEx`).
#[repr(C)]
pub struct OPENFILENAMEA {
    pub lStructSize: DWORD,
    pub hwndOwner: HWND,
    pub hInstance: HINSTANCE,
    pub lpstrFilter: *const u8,
    pub lpstrCustomFilter: *mut u8,
    pub nMaxCustFilter: DWORD,
    pub nFilterIndex: DWORD,
    pub lpstrFile: *mut u8,
    pub nMaxFile: DWORD,
    pub lpstrFileTitle: *mut u8,
    pub nMaxFileTitle: DWORD,
    pub lpstrInitialDir: *const u8,
    pub lpstrTitle: *const u8,
    pub Flags: DWORD,
    pub nFileOffset: WORD,
    pub nFileExtension: WORD,
    pub lpstrDefExt: *const u8,
    pub lCustData: LPARAM,
    pub lpfnHook: *mut c_void,
    pub lpTemplateName: *const u8,
}

#[repr(C)]
pub struct MEMORYSTATUS {
    pub dwLength: DWORD,
    pub dwMemoryLoad: DWORD,
    pub dwTotalPhys: DWORD,
    pub dwAvailPhys: DWORD,
    pub dwTotalPageFile: DWORD,
    pub dwAvailPageFile: DWORD,
    pub dwTotalVirtual: DWORD,
    pub dwAvailVirtual: DWORD,
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleA(lpModuleName: *const u8) -> HMODULE;
    pub fn LoadLibraryA(lpLibFileName: *const u8) -> HMODULE;
    pub fn FreeLibrary(hLibModule: HMODULE) -> BOOL;
    pub fn GetCommandLineA() -> *const u8;
    pub fn GetTickCount() -> DWORD;
    pub fn CreateFileA(
        lpFileName: *const u8,
        dwDesiredAccess: DWORD,
        dwShareMode: DWORD,
        lpSecurityAttributes: *mut c_void,
        dwCreationDisposition: DWORD,
        dwFlagsAndAttributes: DWORD,
        hTemplateFile: HANDLE,
    ) -> HANDLE;
    pub fn GetFileSize(hFile: HANDLE, lpFileSizeHigh: *mut DWORD) -> DWORD;
    pub fn ReadFile(
        hFile: HANDLE,
        lpBuffer: *mut u8,
        nNumberOfBytesToRead: DWORD,
        lpNumberOfBytesRead: *mut DWORD,
        lpOverlapped: *mut c_void,
    ) -> BOOL;
    pub fn WriteFile(
        hFile: HANDLE,
        lpBuffer: *const u8,
        nNumberOfBytesToWrite: DWORD,
        lpNumberOfBytesWritten: *mut DWORD,
        lpOverlapped: *mut c_void,
    ) -> BOOL;
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;
    pub fn GlobalMemoryStatus(lpBuffer: *mut MEMORYSTATUS);
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassA(lpWndClass: *const WNDCLASSA) -> ATOM;
    pub fn CreateWindowExA(
        dwExStyle: DWORD,
        lpClassName: *const u8,
        lpWindowName: *const u8,
        dwStyle: DWORD,
        x: i32,
        y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: HWND,
        hMenu: HMENU,
        hInstance: HINSTANCE,
        lpParam: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcA(hwnd: HWND, msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    pub fn ShowWindow(hwnd: HWND, nCmdShow: i32) -> BOOL;
    pub fn UpdateWindow(hwnd: HWND) -> BOOL;
    pub fn DestroyWindow(hwnd: HWND) -> BOOL;
    pub fn GetMessageA(lpMsg: *mut MSG, hwnd: HWND, wMsgFilterMin: UINT, wMsgFilterMax: UINT) -> BOOL;
    pub fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
    pub fn DispatchMessageA(lpMsg: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(nExitCode: i32);
    pub fn PostMessageA(hwnd: HWND, msg: UINT, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    pub fn SendMessageA(hwnd: HWND, msg: UINT, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    pub fn GetClientRect(hwnd: HWND, lpRect: *mut RECT) -> BOOL;
    pub fn MoveWindow(hwnd: HWND, x: i32, y: i32, nWidth: i32, nHeight: i32, bRepaint: BOOL) -> BOOL;
    pub fn InvalidateRect(hwnd: HWND, lpRect: *const RECT, bErase: BOOL) -> BOOL;
    pub fn BeginPaint(hwnd: HWND, lpPaint: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hwnd: HWND, lpPaint: *const PAINTSTRUCT) -> BOOL;
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
    pub fn FillRect(hdc: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32;
    pub fn DrawFrameControl(hdc: HDC, lprc: *mut RECT, uType: UINT, uState: UINT) -> BOOL;
    pub fn LoadCursorA(hInstance: HINSTANCE, lpCursorName: *const u8) -> HCURSOR;
    pub fn LoadIconA(hInstance: HINSTANCE, lpIconName: *const u8) -> HICON;
    pub fn SetCursor(hCursor: HCURSOR) -> HCURSOR;
    pub fn SetCapture(hwnd: HWND) -> HWND;
    pub fn ReleaseCapture() -> BOOL;
    pub fn SetWindowLongA(hwnd: HWND, nIndex: i32, dwNewLong: isize) -> isize;
    pub fn GetWindowLongA(hwnd: HWND, nIndex: i32) -> isize;
    pub fn SetWindowTextA(hwnd: HWND, lpString: *const u8) -> BOOL;
    pub fn SetTimer(hwnd: HWND, nIDEvent: usize, uElapse: UINT, lpTimerFunc: *mut c_void) -> usize;
    pub fn KillTimer(hwnd: HWND, uIDEvent: usize) -> BOOL;
    pub fn SetScrollInfo(hwnd: HWND, fnBar: i32, lpsi: *const SCROLLINFO, fRedraw: BOOL) -> i32;
    pub fn GetScrollInfo(hwnd: HWND, fnBar: i32, lpsi: *mut SCROLLINFO) -> BOOL;
    pub fn CreateMenu() -> HMENU;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuA(hMenu: HMENU, uFlags: UINT, uIDNewItem: usize, lpNewItem: *const u8) -> BOOL;
    pub fn SetMenu(hwnd: HWND, hMenu: HMENU) -> BOOL;
    pub fn CheckMenuItem(hMenu: HMENU, uIDCheckItem: UINT, uCheck: UINT) -> DWORD;
    pub fn MessageBoxA(hwnd: HWND, lpText: *const u8, lpCaption: *const u8, uType: UINT) -> i32;
    pub fn SetFocus(hwnd: HWND) -> HWND;
    pub fn GetKeyState(nVirtKey: i32) -> i16;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(hdc: HDC) -> HDC;
    pub fn DeleteDC(hdc: HDC) -> BOOL;
    pub fn CreateCompatibleBitmap(hdc: HDC, cx: i32, cy: i32) -> HBITMAP;
    pub fn SelectObject(hdc: HDC, h: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(ho: HGDIOBJ) -> BOOL;
    pub fn GetStockObject(i: i32) -> HGDIOBJ;
    pub fn BitBlt(
        hdcDest: HDC,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        hdcSrc: HDC,
        x1: i32,
        y1: i32,
        rop: DWORD,
    ) -> BOOL;
    pub fn CreateFontIndirectA(lplf: *const LOGFONTA) -> HFONT;
    pub fn CreateSolidBrush(color: COLORREF) -> HBRUSH;
    pub fn CreatePen(iStyle: i32, cWidth: i32, color: COLORREF) -> HPEN;
    pub fn SetTextColor(hdc: HDC, color: COLORREF) -> COLORREF;
    pub fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    pub fn TextOutA(hdc: HDC, x: i32, y: i32, lpString: *const u8, c: i32) -> BOOL;
    pub fn GetTextExtentExPointA(
        hdc: HDC,
        lpszString: *const u8,
        cchString: i32,
        nMaxExtent: i32,
        lpnFit: *mut i32,
        lpnDx: *mut i32,
        lpSize: *mut SIZE,
    ) -> BOOL;
    pub fn MoveToEx(hdc: HDC, x: i32, y: i32, lppt: *mut POINT) -> BOOL;
    pub fn LineTo(hdc: HDC, x: i32, y: i32) -> BOOL;
    pub fn Rectangle(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32) -> BOOL;
}

#[link(name = "comdlg32")]
extern "system" {
    pub fn GetOpenFileNameA(lpofn: *mut OPENFILENAMEA) -> BOOL;
    pub fn GetSaveFileNameA(lpofn: *mut OPENFILENAMEA) -> BOOL;
}

#[link(name = "comctl32")]
extern "system" {
    pub fn InitCommonControlsEx(lpInitCtrls: *const INITCOMMONCONTROLSEX) -> BOOL;
}

#[inline]
pub fn loword(l: usize) -> u16 {
    (l & 0xFFFF) as u16
}

#[inline]
pub fn hiword(l: usize) -> u16 {
    ((l >> 16) & 0xFFFF) as u16
}

#[inline]
pub fn get_x_lparam(lp: LPARAM) -> i32 {
    (lp as u32 & 0xFFFF) as i16 as i32
}

#[inline]
pub fn get_y_lparam(lp: LPARAM) -> i32 {
    ((lp as u32 >> 16) & 0xFFFF) as i16 as i32
}
