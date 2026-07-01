use super::types::FaceFeatureSampleDto;
use crate::core::multimodal::service::{choose_video_source, list_avfoundation_sources};
use crate::core::multimodal::{mediapipe_runtime_available, run_mediapipe_face_features};
use std::fs;
use std::path::Path;
use tokio::process::Command;

pub(crate) async fn resolve_camera_choice(
    preferred_camera_id: Option<&str>,
) -> Result<(String, i32, String), String> {
    let sources = list_avfoundation_sources().await?;
    if let Some(camera_id) = preferred_camera_id {
        let index = parse_camera_id(camera_id)?;
        let source = sources
            .video
            .iter()
            .find(|source| source.index == index)
            .ok_or_else(|| format!("Selected camera is no longer available: {camera_id}"))?;
        return Ok((camera_id.to_string(), source.index, source.name.clone()));
    }

    let source = choose_video_source(&sources.video)
        .ok_or_else(|| "No local camera source is available for gaze calibration.".to_string())?;
    Ok((format!("camera:{}", source.index), source.index, source.name.clone()))
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
            "-pixel_format",
            "bgr0",
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

fn parse_camera_id(camera_id: &str) -> Result<i32, String> {
    camera_id
        .strip_prefix("camera:")
        .ok_or_else(|| "Unknown camera identifier.".to_string())?
        .parse::<i32>()
        .map_err(|e| format!("Failed to parse camera identifier: {e}"))
}
