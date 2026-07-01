use crate::models::capture::{CaptureError, CaptureResult, Display, PixelFormat, RawFrame};

pub(super) async fn get_displays_x11() -> CaptureResult<Vec<Display>> {
    unsafe {
        let display = x11::xlib::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return Err(CaptureError::CaptureFailed(
                "Failed to open X11 display".to_string(),
            ));
        }

        let screen = x11::xlib::XDefaultScreen(display);
        let root = x11::xlib::XRootWindow(display, screen);

        let mut root_return = 0;
        let mut x = 0;
        let mut y = 0;
        let mut width = 0u32;
        let mut height = 0u32;
        let mut border = 0u32;
        let mut depth = 0u32;
        x11::xlib::XGetGeometry(
            display,
            root,
            &mut root_return,
            &mut x,
            &mut y,
            &mut width,
            &mut height,
            &mut border,
            &mut depth,
        );

        let mut displays = Vec::new();
        let mut event_base = 0;
        let mut error_base = 0;
        let has_randr =
            x11::xrandr::XRRQueryExtension(display, &mut event_base, &mut error_base) != 0;

        if has_randr {
            let screen_resources = x11::xrandr::XRRGetScreenResources(display, root);
            if !screen_resources.is_null() {
                let noutput = (*screen_resources).noutput;
                for i in 0..noutput {
                    let output = *(*screen_resources).outputs.add(i as usize);
                    let output_info =
                        x11::xrandr::XRRGetOutputInfo(display, screen_resources, output);
                    if !output_info.is_null()
                        && (*output_info).connection == x11::xrandr::RR_Connected as u16
                        && (*output_info).crtc != 0
                    {
                        let crtc_info = x11::xrandr::XRRGetCrtcInfo(
                            display,
                            screen_resources,
                            (*output_info).crtc,
                        );
                        if !crtc_info.is_null() {
                            let name = if !(*output_info).name.is_null() {
                                let name_slice = std::slice::from_raw_parts(
                                    (*output_info).name as *const u8,
                                    (*output_info).nameLen as usize,
                                );
                                String::from_utf8_lossy(name_slice).to_string()
                            } else {
                                format!("Display {}", i)
                            };

                            displays.push(Display {
                                id: i as u32,
                                name: format!(
                                    "{} ({}x{})",
                                    name,
                                    (*crtc_info).width,
                                    (*crtc_info).height
                                ),
                                x: (*crtc_info).x,
                                y: (*crtc_info).y,
                                width: (*crtc_info).width as u32,
                                height: (*crtc_info).height as u32,
                                is_primary: (*crtc_info).x == 0 && (*crtc_info).y == 0,
                            });
                            x11::xrandr::XRRFreeCrtcInfo(crtc_info);
                        }
                    }

                    if !output_info.is_null() {
                        x11::xrandr::XRRFreeOutputInfo(output_info);
                    }
                }
                x11::xrandr::XRRFreeScreenResources(screen_resources);
            }
        }

        if displays.is_empty() {
            displays.push(Display {
                id: 0,
                name: format!("Default Display ({}x{})", width, height),
                x: 0,
                y: 0,
                width,
                height,
                is_primary: true,
            });
        }

        x11::xlib::XCloseDisplay(display);
        Ok(displays)
    }
}

pub(super) async fn capture_frame_x11(_display_id: u32) -> CaptureResult<RawFrame> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    unsafe {
        let display = x11::xlib::XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return Err(CaptureError::CaptureFailed(
                "Failed to open X11 display".to_string(),
            ));
        }

        let screen = x11::xlib::XDefaultScreen(display);
        let root = x11::xlib::XRootWindow(display, screen);
        let mut root_return = 0;
        let mut x = 0;
        let mut y = 0;
        let mut width = 0u32;
        let mut height = 0u32;
        let mut border = 0u32;
        let mut depth = 0u32;
        x11::xlib::XGetGeometry(
            display,
            root,
            &mut root_return,
            &mut x,
            &mut y,
            &mut width,
            &mut height,
            &mut border,
            &mut depth,
        );

        let image = x11::xlib::XGetImage(
            display,
            root,
            0,
            0,
            width,
            height,
            x11::xlib::XAllPlanes(),
            x11::xlib::ZPixmap,
        );
        if image.is_null() {
            x11::xlib::XCloseDisplay(display);
            return Err(CaptureError::CaptureFailed(
                "Failed to capture X11 image".to_string(),
            ));
        }

        let bytes_per_pixel = ((*image).bits_per_pixel / 8) as usize;
        let bytes_per_line = (*image).bytes_per_line as usize;
        let image_data = (*image).data;
        let pixel_count = (width * height) as usize;
        let mut pixel_data = Vec::with_capacity(pixel_count * 4);

        for row in 0..height {
            for col in 0..width {
                let offset = (row as usize * bytes_per_line) + (col as usize * bytes_per_pixel);
                match bytes_per_pixel {
                    4 => {
                        pixel_data.push(*image_data.add(offset));
                        pixel_data.push(*image_data.add(offset + 1));
                        pixel_data.push(*image_data.add(offset + 2));
                        pixel_data.push(*image_data.add(offset + 3));
                    }
                    3 => {
                        pixel_data.push(*image_data.add(offset));
                        pixel_data.push(*image_data.add(offset + 1));
                        pixel_data.push(*image_data.add(offset + 2));
                        pixel_data.push(255);
                    }
                    _ => {
                        x11::xlib::XDestroyImage(image);
                        x11::xlib::XCloseDisplay(display);
                        return Err(CaptureError::CaptureFailed(format!(
                            "Unsupported pixel format: {} bytes per pixel",
                            bytes_per_pixel
                        )));
                    }
                }
            }
        }

        x11::xlib::XDestroyImage(image);
        x11::xlib::XCloseDisplay(display);

        Ok(RawFrame {
            timestamp,
            width,
            height,
            data: pixel_data,
            format: PixelFormat::BGRA8,
        })
    }
}
