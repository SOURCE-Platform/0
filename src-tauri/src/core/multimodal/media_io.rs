use crate::core::gaze::FaceFeatureSampleDto;
use crate::core::storage::RecordingStorage;
use crate::models::capture::{PixelFormat, RawFrame};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaPipeGazeOutput {
    pub available: bool,
    pub confidence: f32,
    pub vector: Option<serde_json::Value>,
    pub yaw_degrees: Option<f32>,
    pub pitch_degrees: Option<f32>,
    pub model_name: Option<String>,
    pub model_version: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaPipePoseOutput {
    pub person_count: i64,
    pub presence_label: String,
    pub presence_confidence: f32,
    pub posture_label: String,
    pub posture_confidence: f32,
    pub body_bbox: Option<serde_json::Value>,
    pub landmarks: serde_json::Value,
    pub world_landmarks: serde_json::Value,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaPipeSceneOutput {
    pub pose: MediaPipePoseOutput,
    pub face_iris: FaceFeatureSampleDto,
    pub gaze: MediaPipeGazeOutput,
}

pub(super) async fn capture_camera_frame(
    video_index: i32,
    output_path: &Path,
) -> Result<(), String> {
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
        .map_err(|e| format!("Failed to launch ffmpeg for camera frame capture: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("ffmpeg camera capture exited unsuccessfully".to_string())
    }
}

pub(super) async fn capture_audio_chunk(
    audio_index: i32,
    duration_secs: f32,
    output_path: &Path,
) -> Result<(), String> {
    let input = format!(":{audio_index}");
    let duration = format!("{duration_secs}");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "avfoundation",
            "-i",
            &input,
            "-t",
            &duration,
            "-ac",
            "1",
            "-ar",
            "16000",
            "-y",
        ])
        .arg(output_path)
        .status()
        .await
        .map_err(|e| format!("Failed to launch ffmpeg for audio capture: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("ffmpeg audio capture exited unsuccessfully".to_string())
    }
}

pub(super) fn load_png_as_frame(path: &Path) -> Result<RawFrame, String> {
    let image = image::open(path).map_err(|e| format!("Failed to decode frame: {e}"))?;
    let rgba = image.to_rgba8();
    Ok(RawFrame {
        timestamp: chrono::Utc::now().timestamp_millis(),
        width: rgba.width(),
        height: rgba.height(),
        data: rgba.into_raw(),
        format: PixelFormat::RGBA8,
    })
}

pub(crate) async fn mediapipe_runtime_available() -> Result<(), String> {
    let output = Command::new("python3")
        .args([
            "-c",
            "import mediapipe, cv2, onnxruntime; print(mediapipe.__version__)",
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to launch python3 for MediaPipe inspection: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

async fn ensure_mediapipe_runtime_files() -> Result<PathBuf, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "Could not resolve home directory for MediaPipe helper.".to_string())?;
    let helpers_dir = PathBuf::from(home).join(".observer_data").join("helpers");
    fs::create_dir_all(&helpers_dir).map_err(|e| format!("Failed to create helpers dir: {e}"))?;

    let script_path = helpers_dir.join("mediapipe_visual_runtime.py");
    let math_path = helpers_dir.join("mediapipe_visual_math.py");
    let estimator_path = helpers_dir.join("onnx_gaze_estimator.py");
    write_helper_if_needed(
        &script_path,
        include_str!("../helpers/mediapipe_visual_runtime.py"),
    )?;
    write_helper_if_needed(
        &math_path,
        include_str!("../helpers/mediapipe_visual_math.py"),
    )?;
    write_helper_if_needed(
        &estimator_path,
        include_str!("../helpers/onnx_gaze_estimator.py"),
    )?;
    Ok(script_path)
}

fn write_helper_if_needed(path: &Path, contents: &str) -> Result<(), String> {
    let should_write = match fs::read_to_string(path) {
        Ok(existing) => existing != contents,
        Err(_) => true,
    };
    if should_write {
        fs::write(path, contents).map_err(|e| format!("Failed to write MediaPipe helper: {e}"))?;
    }
    Ok(())
}

pub(super) async fn run_mediapipe_scene_inference(
    image_path: &Path,
) -> Result<MediaPipeSceneOutput, String> {
    mediapipe_runtime_available().await?;
    let script_path = ensure_mediapipe_runtime_files().await?;
    let output = Command::new("python3")
        .arg(script_path)
        .arg("scene")
        .arg(image_path)
        .output()
        .await
        .map_err(|e| format!("Failed to run MediaPipe scene helper: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let mut parsed = serde_json::from_slice::<MediaPipeSceneOutput>(&output.stdout)
        .map_err(|e| format!("Failed to parse MediaPipe scene helper output: {e}"))?;
    parsed.face_iris.gaze_vector = parsed.gaze.vector.clone();
    Ok(parsed)
}

pub(crate) async fn run_mediapipe_face_features(
    image_path: &Path,
) -> Result<FaceFeatureSampleDto, String> {
    mediapipe_runtime_available().await?;
    let script_path = ensure_mediapipe_runtime_files().await?;
    let output = Command::new("python3")
        .arg(script_path)
        .arg("face_iris")
        .arg(image_path)
        .output()
        .await
        .map_err(|e| format!("Failed to run MediaPipe face helper: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    serde_json::from_slice::<FaceFeatureSampleDto>(&output.stdout)
        .map_err(|e| format!("Failed to parse MediaPipe face helper output: {e}"))
}

pub(super) async fn save_visual_evidence_frame(
    storage: &RecordingStorage,
    session_id: Uuid,
    frame: &RawFrame,
) -> Result<String, String> {
    let dir = storage
        .get_session_dir(&session_id)
        .join("vision")
        .join("frames");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create vision evidence dir: {e}"))?;
    let path = dir.join(format!("{}.png", frame.timestamp));
    save_raw_frame_png(frame, &path)?;
    Ok(path.to_string_lossy().to_string())
}

pub(super) async fn save_audio_evidence_chunk(
    storage: &RecordingStorage,
    session_id: Uuid,
    temp_path: &Path,
) -> Result<String, String> {
    let dir = storage
        .get_session_dir(&session_id)
        .join("audio")
        .join("chunks");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create audio evidence dir: {e}"))?;
    let target = dir.join(
        temp_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("chunk.wav"),
    );
    fs::copy(temp_path, &target).map_err(|e| format!("Failed to persist audio evidence: {e}"))?;
    Ok(target.to_string_lossy().to_string())
}

fn save_raw_frame_png(frame: &RawFrame, path: &Path) -> Result<(), String> {
    let rgba = match frame.format {
        PixelFormat::RGBA8 => frame.data.clone(),
        PixelFormat::BGRA8 => {
            let mut converted = Vec::with_capacity(frame.data.len());
            for chunk in frame.data.chunks_exact(4) {
                converted.push(chunk[2]);
                converted.push(chunk[1]);
                converted.push(chunk[0]);
                converted.push(chunk[3]);
            }
            converted
        }
    };
    let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(frame.width, frame.height, rgba)
        .ok_or_else(|| "Failed to build RGBA image buffer.".to_string())?;
    image
        .save(path)
        .map_err(|e| format!("Failed to save evidence frame: {e}"))
}
