#![cfg(target_os = "linux")]

use crate::models::activity::AppInfo;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

pub fn detect_display_server() -> DisplayServer {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        DisplayServer::Wayland
    } else if std::env::var("DISPLAY").is_ok() {
        DisplayServer::X11
    } else {
        DisplayServer::Unknown
    }
}

pub fn get_process_info_from_proc(pid: i32) -> Option<AppInfo> {
    match procfs::process::Process::new(pid) {
        Ok(process) => {
            let name = process.stat.ok()?.comm;
            let executable_path = process.exe().ok()?.to_string_lossy().to_string();
            let bundle_id = executable_path.clone();

            Some(AppInfo::with_details(
                name,
                bundle_id,
                pid as u32,
                None,
                Some(executable_path),
            ))
        }
        Err(_) => None,
    }
}

pub fn is_gui_app(process: &procfs::process::Process) -> bool {
    if let Ok(environ) = process.environ() {
        environ.contains_key("DISPLAY") || environ.contains_key("WAYLAND_DISPLAY")
    } else {
        false
    }
}

pub fn get_gui_processes() -> Vec<AppInfo> {
    let mut apps = Vec::new();

    if let Ok(all_procs) = procfs::process::all_processes() {
        for proc_result in all_procs {
            if let Ok(process) = proc_result {
                if is_gui_app(&process) {
                    if let Some(app_info) = get_process_info_from_proc(process.pid) {
                        if !app_info.name.is_empty()
                            && !app_info.name.starts_with("systemd")
                            && !app_info.name.starts_with("dbus")
                        {
                            apps.push(app_info);
                        }
                    }
                }
            }
        }
    }

    apps
}

pub fn get_active_window_pid_x11() -> Option<u32> {
    use std::ptr;
    use x11::xlib::*;

    unsafe {
        let display = XOpenDisplay(ptr::null());
        if display.is_null() {
            return None;
        }

        let root = XDefaultRootWindow(display);
        let active_window_atom =
            XInternAtom(display, b"_NET_ACTIVE_WINDOW\0".as_ptr() as *const i8, 0);

        let mut actual_type = 0;
        let mut actual_format = 0;
        let mut nitems = 0;
        let mut bytes_after = 0;
        let mut prop: *mut u8 = ptr::null_mut();

        let status = XGetWindowProperty(
            display,
            root,
            active_window_atom,
            0,
            1,
            0,
            0,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );

        if status != 0 || prop.is_null() || nitems == 0 {
            XCloseDisplay(display);
            return None;
        }

        let window = *(prop as *const u64);
        XFree(prop as *mut _);

        let pid_atom = XInternAtom(display, b"_NET_WM_PID\0".as_ptr() as *const i8, 0);

        let status = XGetWindowProperty(
            display,
            window,
            pid_atom,
            0,
            1,
            0,
            6,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );

        let pid = if status == 0 && !prop.is_null() && nitems > 0 {
            let pid = *(prop as *const u32);
            XFree(prop as *mut _);
            Some(pid)
        } else {
            None
        };

        XCloseDisplay(display);
        pid
    }
}

pub fn get_frontmost_app_linux() -> Option<AppInfo> {
    match detect_display_server() {
        DisplayServer::X11 => {
            get_active_window_pid_x11().and_then(|pid| get_process_info_from_proc(pid as i32))
        }
        DisplayServer::Wayland | DisplayServer::Unknown => None,
    }
}
