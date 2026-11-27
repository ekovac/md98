#![windows_subsystem = "windows"]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod gdi;
#[cfg(windows)]
mod prefs;
#[cfg(windows)]
mod types;
#[cfg(windows)]
mod win32;
#[cfg(windows)]
mod win32_ffi;

#[cfg(not(windows))]
fn main() {
    println!("md98-win32 is cross-compiled for Windows 98 SE via scripts/build_win98.sh.");
}

#[cfg(windows)]
fn main() {
    app::run();
}
