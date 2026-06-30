use crate::models::capture::{PixelFormat, RawFrame};
use image::{ImageBuffer, Rgba};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::storage::{StorageError, StorageResult};

pub(super) async fn calculate_session_size(
    base_path: &Path,
    session_id: &Uuid,
) -> StorageResult<u64> {
    let frames_path = session_path(base_path, session_id).join("frames");
    let mut total_size = 0u64;

    if frames_path.exists() {
        for entry in std::fs::read_dir(frames_path)? {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("png") {
                total_size += entry.metadata()?.len();
            }
        }
    }

    Ok(total_size)
}

pub(super) fn save_frame_as_png(frame: &RawFrame, path: &Path) -> StorageResult<()> {
    let rgba_data = match frame.format {
        PixelFormat::BGRA8 => {
            let mut rgba = Vec::with_capacity(frame.data.len());
            for chunk in frame.data.chunks_exact(4) {
                rgba.push(chunk[2]);
                rgba.push(chunk[1]);
                rgba.push(chunk[0]);
                rgba.push(chunk[3]);
            }
            rgba
        }
        PixelFormat::RGBA8 => frame.data.clone(),
    };

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(frame.width, frame.height, rgba_data)
            .ok_or_else(|| StorageError::Other("Failed to create image buffer".to_string()))?;

    img.save(path)?;
    Ok(())
}

pub(super) fn session_path(base_path: &Path, session_id: &Uuid) -> PathBuf {
    base_path.join(session_id.to_string())
}
