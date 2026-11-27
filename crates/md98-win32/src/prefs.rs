//! RAII Windows Registry wrapper and persistent user preferences (`HKCU\Software\md98`).

#![cfg(windows)]

use crate::types::{ViewMode, WrapWidth};
use crate::win32_ffi::*;
use core::ptr;

const REG_SUBKEY: &[u8] = b"Software\\md98\0";

/// RAII handle around an open Win32 `HKEY`; always closes cleanly on drop.
struct RegKey(HKEY);

impl RegKey {
    fn open_read(subkey: &[u8]) -> Option<Self> {
        let mut hkey: HKEY = ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExA(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
        if status == ERROR_SUCCESS && !hkey.is_null() {
            Some(Self(hkey))
        } else {
            None
        }
    }

    fn create_write(subkey: &[u8]) -> Option<Self> {
        let mut hkey: HKEY = ptr::null_mut();
        let mut disp: DWORD = 0;
        let status = unsafe {
            RegCreateKeyExA(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                ptr::null_mut(),
                &mut hkey,
                &mut disp,
            )
        };
        if status == ERROR_SUCCESS && !hkey.is_null() {
            Some(Self(hkey))
        } else {
            None
        }
    }

    fn read_dword(&self, name: &[u8]) -> Option<DWORD> {
        let mut reg_type: DWORD = 0;
        let mut val: DWORD = 0;
        let mut sz: DWORD = core::mem::size_of::<DWORD>() as DWORD;
        let status = unsafe {
            RegQueryValueExA(
                self.0,
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

    fn write_dword(&self, name: &[u8], val: DWORD) {
        unsafe {
            let _ = RegSetValueExA(
                self.0,
                name.as_ptr(),
                0,
                REG_DWORD,
                &val as *const DWORD as *const u8,
                core::mem::size_of::<DWORD>() as DWORD,
            );
        }
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Prefs {
    pub view_mode: ViewMode,
    pub dark_mode: bool,
    pub sync_scroll: bool,
    pub wrap_width: WrapWidth,
    pub live_wrap: bool,
    pub reflow_on_save: bool,
    pub split_pos: i32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            view_mode: ViewMode::Split,
            dark_mode: false,
            sync_scroll: true,
            wrap_width: WrapWidth::Cols80,
            live_wrap: false,
            reflow_on_save: false,
            split_pos: 360,
        }
    }
}

impl Prefs {
    pub fn load() -> Self {
        let mut prefs = Self::default();
        let Some(key) = RegKey::open_read(REG_SUBKEY) else {
            return prefs;
        };
        if let Some(v) = key.read_dword(b"ViewMode\0") {
            prefs.view_mode = ViewMode::from_dword(v);
        }
        if let Some(v) = key.read_dword(b"DarkMode\0") {
            prefs.dark_mode = v != 0;
        }
        if let Some(v) = key.read_dword(b"SyncScroll\0") {
            prefs.sync_scroll = v != 0;
        }
        if let Some(v) = key.read_dword(b"WrapWidth\0").and_then(WrapWidth::from_u32) {
            prefs.wrap_width = v;
        }
        if let Some(v) = key.read_dword(b"LiveWrap\0") {
            prefs.live_wrap = v != 0;
        }
        if let Some(v) = key.read_dword(b"ReflowOnSave\0") {
            prefs.reflow_on_save = v != 0;
        }
        if let Some(v) = key.read_dword(b"SplitPos\0") {
            if (100..=2000).contains(&v) {
                prefs.split_pos = v as i32;
            }
        }
        prefs
    }

    pub fn save(&self) {
        let Some(key) = RegKey::create_write(REG_SUBKEY) else {
            return;
        };
        key.write_dword(b"ViewMode\0", self.view_mode as DWORD);
        key.write_dword(b"DarkMode\0", u32::from(self.dark_mode));
        key.write_dword(b"SyncScroll\0", u32::from(self.sync_scroll));
        key.write_dword(b"WrapWidth\0", self.wrap_width as DWORD);
        key.write_dword(b"LiveWrap\0", u32::from(self.live_wrap));
        key.write_dword(b"ReflowOnSave\0", u32::from(self.reflow_on_save));
        if self.split_pos > 0 {
            key.write_dword(b"SplitPos\0", self.split_pos as DWORD);
        }
    }
}
