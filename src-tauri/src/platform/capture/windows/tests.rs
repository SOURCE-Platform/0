#[cfg(test)]
mod tests {
    use super::super::WindowsScreenCapture;
    use crate::models::capture::CaptureError;

    #[tokio::test]
    async fn test_get_displays() {
        let displays = WindowsScreenCapture::get_displays().await.unwrap();
        assert!(!displays.is_empty());
    }

    #[tokio::test]
    async fn test_capture_frame() {
        let displays = WindowsScreenCapture::get_displays().await.unwrap();
        let primary_display = displays
            .iter()
            .find(|d| d.is_primary)
            .unwrap_or(&displays[0]);
        match WindowsScreenCapture::capture_frame(primary_display.id).await {
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
        let displays = WindowsScreenCapture::get_displays().await.unwrap();
        let primary_display = displays
            .iter()
            .find(|d| d.is_primary)
            .unwrap_or(&displays[0]);
        let mut capture = WindowsScreenCapture::new().await.unwrap();
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
