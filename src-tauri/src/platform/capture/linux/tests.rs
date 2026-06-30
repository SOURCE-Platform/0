#[cfg(test)]
mod tests {
    use super::super::{DisplayServer, LinuxScreenCapture};
    use crate::models::capture::CaptureError;

    #[test]
    fn test_display_server_detection() {
        match DisplayServer::detect() {
            DisplayServer::X11 | DisplayServer::Wayland | DisplayServer::Unknown => {}
        }
    }

    #[tokio::test]
    async fn test_get_displays() {
        match LinuxScreenCapture::get_displays().await {
            Ok(displays) => assert!(!displays.is_empty()),
            Err(e) => eprintln!("Failed to get displays: {}", e),
        }
    }

    #[tokio::test]
    async fn test_capture_frame() {
        let displays = match LinuxScreenCapture::get_displays().await {
            Ok(displays) if !displays.is_empty() => displays,
            _ => return,
        };

        let primary_display = displays
            .iter()
            .find(|d| d.is_primary)
            .unwrap_or(&displays[0]);
        match LinuxScreenCapture::capture_frame(primary_display.id).await {
            Ok(frame) => {
                assert!(frame.width > 0);
                assert!(frame.height > 0);
                assert_eq!(frame.data.len(), (frame.width * frame.height * 4) as usize);
            }
            Err(e) => eprintln!("Failed to capture frame: {}", e),
        }
    }

    #[tokio::test]
    async fn test_capture_lifecycle() {
        let displays = match LinuxScreenCapture::get_displays().await {
            Ok(displays) if !displays.is_empty() => displays,
            _ => return,
        };
        let primary_display = displays
            .iter()
            .find(|d| d.is_primary)
            .unwrap_or(&displays[0]);
        let mut capture = match LinuxScreenCapture::new().await {
            Ok(capture) => capture,
            Err(_) => return,
        };

        assert!(!capture.is_capturing());
        capture.start_capture(primary_display.id).await.unwrap();
        assert!(capture.is_capturing());
        assert_eq!(capture.current_display_id(), Some(primary_display.id));
        assert!(matches!(
            capture.start_capture(primary_display.id).await,
            Err(CaptureError::AlreadyCapturing)
        ));
        capture.stop_capture().await.unwrap();
        assert!(!capture.is_capturing());
        assert!(matches!(
            capture.stop_capture().await,
            Err(CaptureError::NotCapturing)
        ));
    }
}
