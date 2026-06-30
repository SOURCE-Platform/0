#[cfg(test)]
mod tests {
    use super::super::{create_screen_capture, ScreenRecorder};
    use crate::core::consent::{ConsentManager, Feature};
    use crate::core::database::Database;
    use crate::core::ocr_trigger_signals::OcrTriggerSignals;
    use crate::core::storage::RecordingStorage;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_create_screen_capture() {
        if let Ok(capture) = create_screen_capture().await {
            if let Ok(displays) = capture.get_displays().await {
                assert!(!displays.is_empty());
            }
        }
    }

    #[tokio::test]
    async fn test_screen_recorder() {
        let db = Arc::new(Database::init().await.unwrap());
        let consent_manager = Arc::new(ConsentManager::new(db.clone()).await.unwrap());
        let temp_dir = std::env::temp_dir().join("observer_test_recordings");
        let storage = Arc::new(
            RecordingStorage::new(temp_dir.clone(), db.clone())
                .await
                .unwrap(),
        );

        let recorder = match ScreenRecorder::new(
            consent_manager.clone(),
            storage.clone(),
            Arc::new(OcrTriggerSignals::new()),
        )
        .await
        {
            Ok(recorder) => recorder,
            Err(_) => return,
        };

        if let Ok(displays) = recorder.get_available_displays().await {
            assert!(!displays.is_empty());
            assert!(!recorder.get_status().await.unwrap().is_recording);
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_recording_lifecycle() {
        use tokio::time::{sleep, Duration};
        let db = Arc::new(Database::init().await.unwrap());
        let consent_manager = Arc::new(ConsentManager::new(db.clone()).await.unwrap());
        let temp_dir = std::env::temp_dir().join("observer_test_lifecycle");
        let storage = Arc::new(
            RecordingStorage::new(temp_dir.clone(), db.clone())
                .await
                .unwrap(),
        );
        consent_manager
            .grant_consent(Feature::ScreenRecording)
            .await
            .unwrap();

        let recorder =
            match ScreenRecorder::new(consent_manager, storage, Arc::new(OcrTriggerSignals::new()))
                .await
            {
                Ok(recorder) => recorder,
                Err(_) => return,
            };

        let displays = match recorder.get_available_displays().await {
            Ok(displays) if !displays.is_empty() => displays,
            _ => return,
        };
        if recorder.start_recording(displays[0].id).await.is_err() {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return;
        }

        assert!(recorder.is_recording().await);
        sleep(Duration::from_secs(2)).await;
        recorder.stop_recording().await.unwrap();
        assert!(!recorder.is_recording().await);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let db = Arc::new(Database::init().await.unwrap());
        let consent_manager = Arc::new(ConsentManager::new(db.clone()).await.unwrap());
        let temp_dir = std::env::temp_dir().join("observer_test_pause");
        let storage = Arc::new(
            RecordingStorage::new(temp_dir.clone(), db.clone())
                .await
                .unwrap(),
        );
        consent_manager
            .grant_consent(Feature::ScreenRecording)
            .await
            .unwrap();

        let recorder =
            match ScreenRecorder::new(consent_manager, storage, Arc::new(OcrTriggerSignals::new()))
                .await
            {
                Ok(recorder) => recorder,
                Err(_) => return,
            };

        let displays = match recorder.get_available_displays().await {
            Ok(displays) if !displays.is_empty() => displays,
            _ => return,
        };
        if recorder.start_recording(displays[0].id).await.is_err() {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        recorder.pause_recording().await.unwrap();
        assert!(recorder.get_status().await.unwrap().is_paused);
        recorder.resume_recording().await.unwrap();
        assert!(!recorder.get_status().await.unwrap().is_paused);
        recorder.stop_recording().await.unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
