use super::types::FaceFeatureSampleDto;
use crate::core::multimodal::service::{choose_video_source, list_avfoundation_sources};
use crate::core::multimodal::{mediapipe_runtime_available, run_mediapipe_face_features};
use std::fs;
use std::path::Path;
use tokio::process::Command;

pub(crate) async fn choose_default_camera() -> Result<(String, i32), String> {
    let sources = list_avfoundation_sources().await?;
    let source = choose_video_source(&sources.video)
        .ok_or_else(|| "No local camera source is available for gaze calibration.".to_string())?;
    Ok((format!("camera:{}", source.index), source.index))
}

pub(crate) async fn capture_face_features_from_camera(
    video_index: i32,
) -> Result<FaceFeatureSampleDto, String> {
    let temp_dir = std::env::temp_dir().join("source_gaze_samples");
    fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create gaze temp dir: {e}"))?;
    let path = temp_dir.join(format!("gaze-{}.png", uuid::Uuid::new_v4()));
    capture_camera_frame(video_index, &path).await?;
    let result = run_mediapipe_face_features(&path).await;
    let _ = fs::remove_file(&path);
    result
}

async fn capture_camera_frame(video_index: i32, output_path: &Path) -> Result<(), String> {
    let input = format!("{video_index}:none");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "avfoundation",
            "-framerate",
            "30",
            "-i",
            &input,
            "-frames:v",
            "1",
            "-y",
        ])
        .arg(output_path)
        .status()
        .await
        .map_err(|e| format!("Failed to launch ffmpeg for gaze frame capture: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("ffmpeg gaze frame capture exited unsuccessfully".to_string())
    }
}

pub(crate) async fn run_gaze_face(image_path: &Path) -> Result<FaceFeatureSampleDto, String> {
    mediapipe_runtime_available().await?;
    run_mediapipe_face_features(image_path).await
}
