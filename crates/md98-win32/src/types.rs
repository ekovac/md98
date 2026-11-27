//! Strongly-typed enums replacing raw Win32 integer IDs, flags, and config magic numbers.

#![cfg(windows)]

use crate::win32_ffi::*;

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlId {
    Edit = 1001,
    Preview = 1002,
    Status = 1003,
}

impl ControlId {
    pub fn as_hmenu(self) -> HMENU {
        self as usize as HMENU
    }
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuId {
    FileNew = 2001,
    FileOpen = 2002,
    FileSave = 2003,
    FileSaveAs = 2004,
    FileExit = 2005,

    EditUndo = 2101,
    EditCut = 2102,
    EditCopy = 2103,
    EditPaste = 2104,
    EditSelectAll = 2105,
    EditFind = 2106,
    EditFindNext = 2107,
    EditReplace = 2108,
    EditReflow = 2109,
    EditLiveWrap = 2110,
    EditReflowOnSave = 2111,
    EditWrap72 = 2112,
    EditWrap80 = 2113,
    EditWrap100 = 2114,

    ViewSplit = 2201,
    ViewEditor = 2202,
    ViewPreview = 2203,
    ViewSyncScroll = 2204,
    ViewDarkMode = 2205,
    ViewToggleF11 = 2206,

    HelpAbout = 2301,
}

impl MenuId {
    pub fn from_raw(id: u16) -> Option<Self> {
        match id as usize {
            2001 => Some(Self::FileNew),
            2002 => Some(Self::FileOpen),
            2003 => Some(Self::FileSave),
            2004 => Some(Self::FileSaveAs),
            2005 => Some(Self::FileExit),
            2101 => Some(Self::EditUndo),
            2102 => Some(Self::EditCut),
            2103 => Some(Self::EditCopy),
            2104 => Some(Self::EditPaste),
            2105 => Some(Self::EditSelectAll),
            2106 => Some(Self::EditFind),
            2107 => Some(Self::EditFindNext),
            2108 => Some(Self::EditReplace),
            2109 => Some(Self::EditReflow),
            2110 => Some(Self::EditLiveWrap),
            2111 => Some(Self::EditReflowOnSave),
            2112 => Some(Self::EditWrap72),
            2113 => Some(Self::EditWrap80),
            2114 => Some(Self::EditWrap100),
            2201 => Some(Self::ViewSplit),
            2202 => Some(Self::ViewEditor),
            2203 => Some(Self::ViewPreview),
            2204 => Some(Self::ViewSyncScroll),
            2205 => Some(Self::ViewDarkMode),
            2206 => Some(Self::ViewToggleF11),
            2301 => Some(Self::HelpAbout),
            _ => None,
        }
    }
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerId {
    Debounce = 1,
    HeadlessExit = 2,
    Stress = 3,
}

impl TimerId {
    pub fn from_raw(id: usize) -> Option<Self> {
        match id {
            1 => Some(Self::Debounce),
            2 => Some(Self::HeadlessExit),
            3 => Some(Self::Stress),
            _ => None,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Split = 0,
    EditorOnly = 1,
    PreviewOnly = 2,
}

impl Default for ViewMode {
    fn default() -> Self {
        Self::Split
    }
}

impl ViewMode {
    pub fn from_dword(v: DWORD) -> Self {
        match v {
            1 => Self::EditorOnly,
            2 => Self::PreviewOnly,
            _ => Self::Split,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::EditorOnly => "editor",
            Self::PreviewOnly => "preview",
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapWidth {
    Cols72 = 72,
    Cols80 = 80,
    Cols100 = 100,
}

impl Default for WrapWidth {
    fn default() -> Self {
        Self::Cols80
    }
}

impl WrapWidth {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            72 => Some(Self::Cols72),
            80 => Some(Self::Cols80),
            100 => Some(Self::Cols100),
            _ => None,
        }
    }

    pub fn columns(self) -> usize {
        self as usize
    }
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollBar {
    Horz = SB_HORZ,
    Vert = SB_VERT,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAction {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
    Top,
    Bottom,
    ThumbTrack,
    Other,
}

impl ScrollAction {
    pub fn from_wparam(wparam: WPARAM) -> Self {
        match loword(wparam) {
            SB_LINEUP => Self::LineUp,
            SB_LINEDOWN => Self::LineDown,
            SB_PAGEUP => Self::PageUp,
            SB_PAGEDOWN => Self::PageDown,
            SB_TOP => Self::Top,
            SB_BOTTOM => Self::Bottom,
            SB_THUMBTRACK | SB_THUMBPOSITION => Self::ThumbTrack,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageIcon {
    Info,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavePromptChoice {
    Save,
    Discard,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindReplaceAction {
    DialogClosed,
    FindNext,
    Replace,
    ReplaceAll,
    None,
}

impl FindReplaceAction {
    pub fn from_flags(flags: DWORD) -> Self {
        if (flags & FR_DIALOGTERM) != 0 {
            Self::DialogClosed
        } else if (flags & FR_FINDNEXT) != 0 {
            Self::FindNext
        } else if (flags & FR_REPLACE) != 0 {
            Self::Replace
        } else if (flags & FR_REPLACEALL) != 0 {
            Self::ReplaceAll
        } else {
            Self::None
        }
    }
}
