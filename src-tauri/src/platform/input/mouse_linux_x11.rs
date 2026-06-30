#![cfg(target_os = "linux")]

use crate::models::input::{AppContext, Point};

pub(super) fn get_cursor_position_x11() -> Point {
    #[cfg(feature = "x11")]
    unsafe {
        use x11::xlib::*;

        let display = XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return Point { x: 0, y: 0 };
        }

        let root = XDefaultRootWindow(display);
        let mut root_return = 0;
        let mut child_return = 0;
        let mut root_x = 0;
        let mut root_y = 0;
        let mut win_x = 0;
        let mut win_y = 0;
        let mut mask = 0;

        XQueryPointer(
            display,
            root,
            &mut root_return,
            &mut child_return,
            &mut root_x,
            &mut root_y,
            &mut win_x,
            &mut win_y,
            &mut mask,
        );

        XCloseDisplay(display);
        Point {
            x: root_x,
            y: root_y,
        }
    }

    #[cfg(not(feature = "x11"))]
    Point { x: 0, y: 0 }
}

pub(super) fn get_current_app_context() -> AppContext {
    #[cfg(feature = "x11")]
    {
        if let Ok(context) = get_app_context_x11() {
            return context;
        }
    }

    AppContext {
        app_name: String::from("Unknown"),
        window_title: String::from("Unknown"),
        process_id: 0,
    }
}

#[cfg(feature = "x11")]
fn get_app_context_x11() -> Result<AppContext, Box<dyn std::error::Error>> {
    use x11::xlib::*;

    unsafe {
        let display = XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return Err("Failed to open X11 display".into());
        }

        let root = XDefaultRootWindow(display);
        let mut actual_type = 0;
        let mut actual_format = 0;
        let mut nitems = 0;
        let mut bytes_after = 0;
        let mut prop: *mut u8 = std::ptr::null_mut();
        let net_active_window =
            XInternAtom(display, b"_NET_ACTIVE_WINDOW\0".as_ptr() as *const i8, 0);

        XGetWindowProperty(
            display,
            root,
            net_active_window,
            0,
            1,
            0,
            33,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );

        if prop.is_null() {
            XCloseDisplay(display);
            return Err("Failed to get active window".into());
        }

        let window = *(prop as *const u64);
        XFree(prop as *mut _);

        let mut window_title = String::from("Unknown");
        let mut text_prop = std::mem::zeroed();
        if XGetWMName(display, window, &mut text_prop) != 0 && !text_prop.value.is_null() {
            window_title = std::ffi::CStr::from_ptr(text_prop.value as *const i8)
                .to_string_lossy()
                .into_owned();
            XFree(text_prop.value as *mut _);
        }

        let mut process_id = 0u32;
        let net_wm_pid = XInternAtom(display, b"_NET_WM_PID\0".as_ptr() as *const i8, 0);

        XGetWindowProperty(
            display,
            window,
            net_wm_pid,
            0,
            1,
            0,
            19,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );

        if !prop.is_null() {
            process_id = *(prop as *const u32);
            XFree(prop as *mut _);
        }

        let app_name = if process_id > 0 {
            std::fs::read_to_string(format!("/proc/{}/comm", process_id))
                .unwrap_or_else(|_| String::from("Unknown"))
                .trim()
                .to_string()
        } else {
            String::from("Unknown")
        };

        XCloseDisplay(display);

        Ok(AppContext {
            app_name,
            window_title,
            process_id,
        })
    }
}
