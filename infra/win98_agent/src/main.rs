#![windows_subsystem = "windows"]

#[cfg(not(windows))]
fn main() {
    println!("win98_agent is intended to be cross-compiled for Windows 98 SE.");
}

#[cfg(windows)]
mod agent {
    use core::ffi::c_void;

    type HANDLE = *mut c_void;
    type DWORD = u32;
    type WORD = u16;
    type BOOL = i32;
    type LPBYTE = *mut u8;

    const INVALID_HANDLE_VALUE: HANDLE = (-1isize) as HANDLE;
    const GENERIC_READ: DWORD = 0x8000_0000;
    const GENERIC_WRITE: DWORD = 0x4000_0000;
    const OPEN_EXISTING: DWORD = 3;
    const WAIT_OBJECT_0: DWORD = 0;
    const WAIT_TIMEOUT: DWORD = 0x0000_0102;

    #[repr(C)]
    struct DCB {
        dcb_length: DWORD,
        baud_rate: DWORD,
        flags: DWORD,
        w_reserved: WORD,
        xon_lim: WORD,
        xoff_lim: WORD,
        byte_size: u8,
        parity: u8,
        stop_bits: u8,
        xon_char: i8,
        xoff_char: i8,
        error_char: i8,
        eof_char: i8,
        evt_char: i8,
        w_reserved1: WORD,
    }

    #[repr(C)]
    struct COMMTIMEOUTS {
        read_interval_timeout: DWORD,
        read_total_timeout_multiplier: DWORD,
        read_total_timeout_constant: DWORD,
        write_total_timeout_multiplier: DWORD,
        write_total_timeout_constant: DWORD,
    }

    #[repr(C)]
    struct STARTUPINFOA {
        cb: DWORD,
        lp_reserved: *mut u8,
        lp_desktop: *mut u8,
        lp_title: *mut u8,
        dw_x: DWORD,
        dw_y: DWORD,
        dw_x_size: DWORD,
        dw_y_size: DWORD,
        dw_x_count_chars: DWORD,
        dw_y_count_chars: DWORD,
        dw_fill_attribute: DWORD,
        dw_flags: DWORD,
        w_show_window: WORD,
        cb_reserved2: WORD,
        lp_reserved2: LPBYTE,
        h_std_input: HANDLE,
        h_std_output: HANDLE,
        h_std_error: HANDLE,
    }

    #[repr(C)]
    struct PROCESS_INFORMATION {
        h_process: HANDLE,
        h_thread: HANDLE,
        dw_process_id: DWORD,
        dw_thread_id: DWORD,
    }

    #[repr(C)]
    struct MEMORYSTATUS {
        dw_length: DWORD,
        dw_memory_load: DWORD,
        dw_total_phys: DWORD,
        dw_avail_phys: DWORD,
        dw_total_page_file: DWORD,
        dw_avail_page_file: DWORD,
        dw_total_virtual: DWORD,
        dw_avail_virtual: DWORD,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileA(
            lp_file_name: *const u8,
            dw_desired_access: DWORD,
            dw_share_mode: DWORD,
            lp_security_attributes: *mut c_void,
            dw_creation_disposition: DWORD,
            dw_flags_and_attributes: DWORD,
            h_template_file: HANDLE,
        ) -> HANDLE;
        fn GetFileSize(h_file: HANDLE, lp_file_size_high: *mut DWORD) -> DWORD;
        fn CopyFileA(lp_existing_file_name: *const u8, lp_new_file_name: *const u8, b_fail_if_exists: BOOL) -> BOOL;
        fn GetCommState(h_file: HANDLE, lp_dcb: *mut DCB) -> BOOL;
        fn SetCommState(h_file: HANDLE, lp_dcb: *mut DCB) -> BOOL;
        fn SetCommTimeouts(h_file: HANDLE, lp_comm_timeouts: *const COMMTIMEOUTS) -> BOOL;
        fn ReadFile(
            h_file: HANDLE,
            lp_buffer: *mut u8,
            n_number_of_bytes_to_read: DWORD,
            lp_number_of_bytes_read: *mut DWORD,
            lp_overlapped: *mut c_void,
        ) -> BOOL;
        fn WriteFile(
            h_file: HANDLE,
            lp_buffer: *const u8,
            n_number_of_bytes_to_write: DWORD,
            lp_number_of_bytes_written: *mut DWORD,
            lp_overlapped: *mut c_void,
        ) -> BOOL;
        fn FlushFileBuffers(h_file: HANDLE) -> BOOL;
        fn CloseHandle(h_object: HANDLE) -> BOOL;
        fn CreateProcessA(
            lp_application_name: *const u8,
            lp_command_line: *mut u8,
            lp_process_attributes: *mut c_void,
            lp_thread_attributes: *mut c_void,
            b_inherit_handles: BOOL,
            dw_creation_flags: DWORD,
            lp_environment: *mut c_void,
            lp_current_directory: *const u8,
            lp_startup_info: *const STARTUPINFOA,
            lp_process_information: *mut PROCESS_INFORMATION,
        ) -> BOOL;
        fn WaitForSingleObject(h_handle: HANDLE, dw_milliseconds: DWORD) -> DWORD;
        fn GetExitCodeProcess(h_process: HANDLE, lp_exit_code: *mut DWORD) -> BOOL;
        fn TerminateProcess(h_process: HANDLE, u_exit_code: u32) -> BOOL;
        fn GetLastError() -> DWORD;
        fn GetTickCount() -> DWORD;
        fn Sleep(dw_milliseconds: DWORD);
        fn GlobalMemoryStatus(lp_buffer: *mut MEMORYSTATUS);
    }

    fn to_cstr(s: &str) -> Vec<u8> {
        let mut v = Vec::with_capacity(s.len() + 1);
        v.extend_from_slice(s.as_bytes());
        v.push(0);
        v
    }

    fn write_all(h: HANDLE, data: &[u8]) {
        let mut offset = 0usize;
        while offset < data.len() {
            let mut written: DWORD = 0;
            let ok = unsafe {
                WriteFile(
                    h,
                    data.as_ptr().add(offset),
                    (data.len() - offset) as DWORD,
                    &mut written,
                    core::ptr::null_mut(),
                )
            };
            if ok == 0 || written == 0 {
                break;
            }
            offset += written as usize;
        }
        unsafe {
            FlushFileBuffers(h);
        }
    }

    fn write_line(h: HANDLE, msg: &str) {
        write_all(h, msg.as_bytes());
        write_all(h, b"\r\n");
    }

    fn format_mem_status() -> String {
        let mut ms: MEMORYSTATUS = unsafe { core::mem::zeroed() };
        ms.dw_length = core::mem::size_of::<MEMORYSTATUS>() as DWORD;
        unsafe {
            GlobalMemoryStatus(&mut ms);
        }
        format!(
            "MEM load={} total_phys={} avail_phys={} avail_page={} avail_virt={}",
            ms.dw_memory_load,
            ms.dw_total_phys,
            ms.dw_avail_phys,
            ms.dw_avail_page_file,
            ms.dw_avail_virtual
        )
    }

    fn run_command(h_com: HANDLE, cmd: &str, wait_timeout_ms: Option<DWORD>) {
        let mut cmd_bytes = to_cstr(cmd);
        let mut si: STARTUPINFOA = unsafe { core::mem::zeroed() };
        si.cb = core::mem::size_of::<STARTUPINFOA>() as DWORD;
        let mut pi: PROCESS_INFORMATION = unsafe { core::mem::zeroed() };

        let t0 = unsafe { GetTickCount() };
        let ok = unsafe {
            CreateProcessA(
                core::ptr::null(),
                cmd_bytes.as_mut_ptr(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                0,
                0,
                core::ptr::null_mut(),
                core::ptr::null(),
                &si,
                &mut pi,
            )
        };

        if ok == 0 {
            let err = unsafe { GetLastError() };
            write_line(h_com, &format!("ERR CreateProcessA failed gle={}", err));
            return;
        }

        write_line(h_com, &format!("STARTED pid={}", pi.dw_process_id));

        if let Some(timeout_ms) = wait_timeout_ms {
            let wait_res = unsafe { WaitForSingleObject(pi.h_process, timeout_ms) };
            let elapsed = unsafe { GetTickCount().wrapping_sub(t0) };

            if wait_res == WAIT_OBJECT_0 {
                let mut exit_code: DWORD = 0;
                unsafe {
                    GetExitCodeProcess(pi.h_process, &mut exit_code);
                }
                write_line(
                    h_com,
                    &format!(
                        "EXITED pid={} code={} elapsed_ms={}",
                        pi.dw_process_id, exit_code, elapsed
                    ),
                );
            } else if wait_res == WAIT_TIMEOUT {
                unsafe {
                    TerminateProcess(pi.h_process, 124);
                }
                write_line(
                    h_com,
                    &format!("TIMEOUT pid={} elapsed_ms={}", pi.dw_process_id, elapsed),
                );
            } else {
                write_line(
                    h_com,
                    &format!(
                        "WAIT_ERR pid={} res={} elapsed_ms={}",
                        pi.dw_process_id, wait_res, elapsed
                    ),
                );
            }
            write_line(h_com, &format_mem_status());
        }

        unsafe {
            CloseHandle(pi.h_thread);
            CloseHandle(pi.h_process);
        }
    }

    fn read_file_to_serial(h_com: HANDLE, path: &str) {
        let p = to_cstr(path);
        let hf = unsafe {
            CreateFileA(
                p.as_ptr(),
                GENERIC_READ,
                1,
                core::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                core::ptr::null_mut(),
            )
        };
        if hf == INVALID_HANDLE_VALUE {
            let err = unsafe { GetLastError() };
            write_line(h_com, &format!("ERR READ_FILE gle={}", err));
            return;
        }
        write_line(h_com, "BEGIN_FILE");
        let mut buf = [0u8; 512];
        loop {
            let mut n: DWORD = 0;
            let ok = unsafe {
                ReadFile(
                    hf,
                    buf.as_mut_ptr(),
                    buf.len() as DWORD,
                    &mut n,
                    core::ptr::null_mut(),
                )
            };
            if ok == 0 || n == 0 {
                break;
            }
            write_all(h_com, &buf[..n as usize]);
        }
        unsafe {
            CloseHandle(hf);
        }
        write_all(h_com, b"\r\nEND_FILE\r\n");
    }

    fn stat_file(h_com: HANDLE, path: &str) {
        let p = to_cstr(path);
        let hf = unsafe {
            CreateFileA(
                p.as_ptr(),
                GENERIC_READ,
                1,
                core::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                core::ptr::null_mut(),
            )
        };
        if hf == INVALID_HANDLE_VALUE {
            let err = unsafe { GetLastError() };
            write_line(h_com, &format!("ERR STAT gle={}", err));
            return;
        }
        let sz = unsafe { GetFileSize(hf, core::ptr::null_mut()) };
        unsafe {
            CloseHandle(hf);
        }
        write_line(h_com, &format!("STAT size={}", sz));
    }

    fn copy_file(h_com: HANDLE, rest: &str) {
        let mut parts = rest.split_whitespace();
        let (Some(src), Some(dst)) = (parts.next(), parts.next()) else {
            write_line(h_com, "ERR COPY usage");
            return;
        };
        let s = to_cstr(src);
        let d = to_cstr(dst);
        let ok = unsafe { CopyFileA(s.as_ptr(), d.as_ptr(), 0) };
        if ok != 0 {
            write_line(h_com, "OK COPY");
        } else {
            let err = unsafe { GetLastError() };
            write_line(h_com, &format!("ERR COPY gle={}", err));
        }
    }

    fn open_com1() -> HANDLE {
        let name = b"COM1\0";
        loop {
            let h = unsafe {
                CreateFileA(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    core::ptr::null_mut(),
                    OPEN_EXISTING,
                    0,
                    core::ptr::null_mut(),
                )
            };
            if h != INVALID_HANDLE_VALUE {
                let mut dcb: DCB = unsafe { core::mem::zeroed() };
                dcb.dcb_length = core::mem::size_of::<DCB>() as DWORD;
                unsafe {
                    if GetCommState(h, &mut dcb) != 0 {
                        dcb.baud_rate = 115200;
                        dcb.byte_size = 8;
                        dcb.parity = 0;
                        dcb.stop_bits = 0;
                        SetCommState(h, &mut dcb);
                    }
                    let timeouts = COMMTIMEOUTS {
                        read_interval_timeout: 50,
                        read_total_timeout_multiplier: 0,
                        read_total_timeout_constant: 200,
                        write_total_timeout_multiplier: 0,
                        write_total_timeout_constant: 1000,
                    };
                    SetCommTimeouts(h, &timeouts);
                }
                return h;
            }
            unsafe {
                Sleep(500);
            }
        }
    }

    pub fn run() {
        let h_com = open_com1();
        write_line(h_com, "READY win98_agent");
        write_line(h_com, &format_mem_status());

        let mut line = Vec::<u8>::with_capacity(512);
        let mut buf = [0u8; 64];

        loop {
            let mut n: DWORD = 0;
            let ok = unsafe {
                ReadFile(
                    h_com,
                    buf.as_mut_ptr(),
                    buf.len() as DWORD,
                    &mut n,
                    core::ptr::null_mut(),
                )
            };
            if ok == 0 || n == 0 {
                continue;
            }
            for &b in &buf[..n as usize] {
                if b == b'\n' || b == b'\r' {
                    if !line.is_empty() {
                        if let Ok(cmd_str) = core::str::from_utf8(&line) {
                            let trimmed = cmd_str.trim();
                            if trimmed == "PING" {
                                write_line(h_com, "PONG");
                            } else if trimmed == "MEM" {
                                write_line(h_com, &format_mem_status());
                            } else if let Some(rest) = trimmed.strip_prefix("RUN ") {
                                run_command(h_com, rest.trim(), Some(60_000));
                            } else if let Some(rest) = trimmed.strip_prefix("LAUNCH ") {
                                run_command(h_com, rest.trim(), None);
                            } else if let Some(rest) = trimmed.strip_prefix("READ ") {
                                read_file_to_serial(h_com, rest.trim());
                            } else if let Some(rest) = trimmed.strip_prefix("STAT ") {
                                stat_file(h_com, rest.trim());
                            } else if let Some(rest) = trimmed.strip_prefix("COPY ") {
                                copy_file(h_com, rest.trim());
                            } else if !trimmed.is_empty() {
                                write_line(h_com, &format!("ERR UNKNOWN {}", trimmed));
                            }
                        }
                        line.clear();
                    }
                } else if line.len() < 1024 {
                    line.push(b);
                }
            }
        }
    }
}

#[cfg(windows)]
fn main() {
    agent::run();
}
