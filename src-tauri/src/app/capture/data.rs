use crate::app::state::{AppState, CaptureDataChannelUsageDto, CaptureDataOverviewDto};
use crate::core::database::Database;
use crate::core::gaze;
use crate::core::multimodal;
use crate::core::ocr_agent_context;
use crate::platform::get_platform;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

struct CaptureChannelMeta {
    channel: &'static str,
    label: &'static str,
    storage_kind: &'static str,
    count_query: &'static str,
    last_query: &'static str,
}

const CAPTURE_CHANNELS: [CaptureChannelMeta; 9] = [
    CaptureChannelMeta { channel: "system", label: "OS / session events", storage_kind: "database", count_query: "SELECT COUNT(*) FROM context_events WHERE channel = 'system'", last_query: "SELECT MAX(timestamp) FROM context_events WHERE channel = 'system'" },
    CaptureChannelMeta { channel: "focus", label: "Focus and running apps", storage_kind: "database", count_query: "SELECT COUNT(*) FROM context_events WHERE channel = 'focus'", last_query: "SELECT MAX(timestamp) FROM context_events WHERE channel = 'focus'" },
    CaptureChannelMeta { channel: "visible_windows", label: "Visible windows snapshots", storage_kind: "database", count_query: "SELECT COUNT(*) FROM window_snapshots", last_query: "SELECT MAX(timestamp) FROM window_snapshots" },
    CaptureChannelMeta { channel: "keyboard", label: "Keyboard activity", storage_kind: "database", count_query: "SELECT COUNT(*) FROM keyboard_events", last_query: "SELECT MAX(timestamp) FROM keyboard_events" },
    CaptureChannelMeta { channel: "mouse", label: "Mouse activity", storage_kind: "database", count_query: "SELECT COUNT(*) FROM mouse_events", last_query: "SELECT MAX(timestamp) FROM mouse_events" },
    CaptureChannelMeta { channel: "ocr", label: "OCR text capture", storage_kind: "database", count_query: "SELECT COUNT(*) FROM ocr_results", last_query: "SELECT MAX(timestamp) FROM ocr_results" },
    CaptureChannelMeta { channel: "screen_frames", label: "Screen keyframes / evidence", storage_kind: "files + database", count_query: "SELECT COUNT(*) FROM frames", last_query: "SELECT MAX(timestamp) FROM frames" },
    CaptureChannelMeta { channel: "camera_future", label: "Vision / scene", storage_kind: "database", count_query: "SELECT (SELECT COUNT(*) FROM visual_scene_snapshots) + (SELECT COUNT(*) FROM gaze_samples) + (SELECT COUNT(*) FROM attention_snapshots) + (SELECT COUNT(*) FROM attention_spans)", last_query: "SELECT MAX(value) FROM (SELECT MAX(timestamp) AS value FROM visual_scene_snapshots UNION ALL SELECT MAX(timestamp) AS value FROM gaze_samples UNION ALL SELECT MAX(timestamp) AS value FROM attention_snapshots UNION ALL SELECT MAX(last_seen_at) AS value FROM attention_spans)" },
    CaptureChannelMeta { channel: "audio_future", label: "Audio / speech", storage_kind: "files + database", count_query: "SELECT COUNT(*) FROM audio_chunks", last_query: "SELECT MAX(start_timestamp) FROM audio_chunks" },
];

fn file_size(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn directory_size(path: &Path) -> u64 {
    fn walk(path: &Path) -> io::Result<u64> {
        if !path.exists() {
            return Ok(0);
        }
        let metadata = fs::metadata(path)?;
        if metadata.is_file() {
            return Ok(metadata.len());
        }

        let mut total = 0;
        for entry in fs::read_dir(path)? {
            total += walk(&entry?.path())?;
        }
        Ok(total)
    }

    walk(path).unwrap_or(0)
}

fn remove_file_if_exists(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

fn remove_empty_parent_dirs(path: &Path, stop_at: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == stop_at {
            break;
        }
        let is_empty = fs::read_dir(dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            break;
        }
        let _ = fs::remove_dir(dir);
        current = dir.parent();
    }
}

pub async fn ensure_capture_stopped(state: &AppState) -> Result<(), String> {
    if state.desktop_capture_runtime.read().await.is_active {
        Err("Stop capture before deleting data.".to_string())
    } else {
        Ok(())
    }
}

pub fn actual_recordings_path(state: &AppState) -> Result<PathBuf, String> {
    if let Some(storage) = state.storage.as_ref() {
        return Ok(storage.base_path());
    }
    Ok(get_platform()
        .get_data_directory()
        .map_err(|e| format!("Failed to resolve data directory: {}", e))?
        .join("recordings"))
}

#[cfg(target_family = "unix")]
fn get_disk_space_for_path(path: &Path) -> Result<(u64, u64), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "Failed to prepare path for disk inspection.".to_string())?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::statvfs(c_path.as_ptr(), &mut stats) };
    if result != 0 {
        return Err(format!(
            "Failed to inspect disk space: {}",
            std::io::Error::last_os_error()
        ));
    }
    let block_size = stats.f_frsize as u64;
    Ok((
        stats.f_blocks as u64 * block_size,
        stats.f_bavail as u64 * block_size,
    ))
}

#[cfg(not(target_family = "unix"))]
fn get_disk_space_for_path(_path: &Path) -> Result<(u64, u64), String> {
    Err("Disk space inspection is not implemented on this platform.".to_string())
}

pub async fn collect_capture_data_overview(
    state: &AppState,
) -> Result<CaptureDataOverviewDto, String> {
    let database_path =
        Database::database_path().map_err(|e| format!("Failed to resolve database path: {}", e))?;
    let configured_storage_path = state
        .config
        .lock()
        .map_err(|e| format!("Failed to lock config: {}", e))?
        .storage_path
        .clone();
    let configured_storage_path_string = configured_storage_path.to_string_lossy().to_string();
    let actual_recordings_path = actual_recordings_path(state)?;
    let database_size_bytes = file_size(&database_path);
    let recordings_size_bytes = directory_size(&actual_recordings_path);
    let total_size_bytes = database_size_bytes + recordings_size_bytes;
    let disk_probe_path = if actual_recordings_path.exists() {
        actual_recordings_path.as_path()
    } else {
        database_path.parent().unwrap_or_else(|| Path::new("/"))
    };
    let (disk_total_bytes, disk_free_bytes) = get_disk_space_for_path(disk_probe_path)?;
    let disk_used_bytes = disk_total_bytes.saturating_sub(disk_free_bytes);
    let source_percent_of_disk = if disk_total_bytes > 0 {
        total_size_bytes as f32 / disk_total_bytes as f32 * 100.0
    } else {
        0.0
    };
    let source_percent_of_free_space = if disk_free_bytes > 0 {
        total_size_bytes as f32 / disk_free_bytes as f32 * 100.0
    } else {
        0.0
    };
    let disk_health = if disk_free_bytes <= 10 * 1024 * 1024 * 1024 {
        "critical"
    } else if disk_free_bytes <= 25 * 1024 * 1024 * 1024 {
        "warning"
    } else {
        "healthy"
    };
    let disk_warning = match disk_health {
        "critical" => Some("Disk space is critically low. Keep SOURCE lean and delete unused capture data quickly.".to_string()),
        "warning" => Some("Disk space is getting tight. Watch SOURCE storage growth before longer capture sessions.".to_string()),
        _ => None,
    };

    let mut channels = Vec::new();
    for channel in CAPTURE_CHANNELS {
        let row_count: i64 = sqlx::query_scalar(channel.count_query)
            .fetch_one(state.db.pool())
            .await
            .map_err(|e| format!("Failed to inspect {} rows: {}", channel.channel, e))?;
        let last_event_time: Option<i64> = sqlx::query_scalar(channel.last_query)
            .fetch_one(state.db.pool())
            .await
            .map_err(|e| format!("Failed to inspect {} timestamp: {}", channel.channel, e))?;
        channels.push(CaptureDataChannelUsageDto {
            channel: channel.channel.to_string(),
            label: channel.label.to_string(),
            storage_kind: channel.storage_kind.to_string(),
            row_count,
            disk_bytes: if channel.channel == "screen_frames" {
                recordings_size_bytes
            } else {
                0
            },
            last_event_time,
        });
    }

    let mut notes = vec![
        "Most capture channels currently store rows inside the local SOURCE SQLite database.".to_string(),
        "Screen keyframes and encoded evidence segments also write files into the recordings directory.".to_string(),
        "Delete actions are disabled while capture is running so SOURCE does not remove live data out from under an active session.".to_string(),
    ];
    if configured_storage_path != actual_recordings_path {
        notes.push("The configured storage path and the runtime recordings path do not currently match, so the runtime path shown here is the source of truth for captured evidence files.".to_string());
    }

    Ok(CaptureDataOverviewDto {
        database_path: database_path.to_string_lossy().to_string(),
        configured_storage_path: configured_storage_path_string,
        actual_recordings_path: actual_recordings_path.to_string_lossy().to_string(),
        database_size_bytes,
        recordings_size_bytes,
        total_size_bytes,
        disk_total_bytes,
        disk_free_bytes,
        disk_used_bytes,
        source_percent_of_disk,
        source_percent_of_free_space,
        disk_health: disk_health.to_string(),
        disk_warning,
        channels,
        notes,
    })
}

pub async fn clear_screen_evidence_channel(state: &AppState) -> Result<(), String> {
    let recordings_root = actual_recordings_path(state)?;
    let frame_paths: Vec<String> = sqlx::query_scalar("SELECT file_path FROM frames")
        .fetch_all(state.db.pool())
        .await
        .map_err(|e| format!("Failed to list frame paths: {}", e))?;
    let segment_paths: Vec<String> = sqlx::query_scalar("SELECT file_path FROM video_segments")
        .fetch_all(state.db.pool())
        .await
        .map_err(|e| format!("Failed to list segment paths: {}", e))?;
    let base_layer_paths: Vec<Option<String>> =
        sqlx::query_scalar("SELECT base_layer_path FROM screen_recordings")
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to list base-layer paths: {}", e))?;
    let session_dirs: Vec<Option<String>> =
        sqlx::query_scalar("SELECT recording_path FROM sessions")
            .fetch_all(state.db.pool())
            .await
            .map_err(|e| format!("Failed to list recording paths: {}", e))?;

    for path in frame_paths {
        let file_path = PathBuf::from(&path);
        remove_file_if_exists(&file_path);
        remove_empty_parent_dirs(&file_path, &recordings_root);
    }
    for path in segment_paths {
        let file_path = PathBuf::from(&path);
        remove_file_if_exists(&file_path);
        remove_empty_parent_dirs(&file_path, &recordings_root);
    }
    for path in base_layer_paths.into_iter().flatten() {
        let file_path = PathBuf::from(&path);
        remove_file_if_exists(&file_path);
        remove_empty_parent_dirs(&file_path, &recordings_root);
    }

    sqlx::query("DELETE FROM screen_recordings")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to clear screen recordings: {}", e))?;
    sqlx::query("DELETE FROM video_segments")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to clear video segments: {}", e))?;
    sqlx::query("DELETE FROM frames")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to clear frames: {}", e))?;
    sqlx::query(
        "UPDATE sessions
         SET frame_count = 0,
             total_size_bytes = 0,
             segment_count = 0,
             total_motion_percentage = 0.0,
             recording_path = NULL,
             base_layer_path = NULL",
    )
    .execute(state.db.pool())
    .await
    .map_err(|e| format!("Failed to reset session evidence metadata: {}", e))?;

    for session_dir in session_dirs.into_iter().flatten() {
        let session_path = PathBuf::from(session_dir);
        if session_path.exists() {
            let frames_dir = session_path.join("frames");
            let segments_dir = session_path.join("segments");
            if frames_dir.exists() {
                let _ = fs::remove_dir_all(&frames_dir);
            }
            if segments_dir.exists() {
                let _ = fs::remove_dir_all(&segments_dir);
            }
            let _ = fs::remove_file(session_path.join("base_layer.png"));
            let is_empty = fs::read_dir(&session_path)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if is_empty {
                let _ = fs::remove_dir(&session_path);
            }
        }
    }

    Ok(())
}

pub async fn delete_all_capture_data(state: &AppState) -> Result<(), String> {
    let recordings_root = actual_recordings_path(state)?;
    clear_screen_evidence_channel(state).await?;
    sqlx::query("DELETE FROM ocr_results")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to delete OCR results: {}", e))?;
    ocr_agent_context::delete_all_derived(&state.db)
        .await
        .map_err(|e| format!("Failed to delete derived OCR data: {}", e))?;
    multimodal::delete_all_multimodal_derived(&state.db)
        .await
        .map_err(|e| format!("Failed to delete multimodal derived data: {}", e))?;
    gaze::delete_all_gaze_data(&state.db)
        .await
        .map_err(|e| format!("Failed to delete gaze data: {}", e))?;
    for query in [
        "DELETE FROM keyboard_events",
        "DELETE FROM mouse_events",
        "DELETE FROM window_snapshots",
        "DELETE FROM context_events",
        "DELETE FROM sessions",
    ] {
        sqlx::query(query)
            .execute(state.db.pool())
            .await
            .map_err(|e| format!("Failed to delete capture data: {}", e))?;
    }
    if recordings_root.exists() {
        let _ = fs::remove_dir_all(&recordings_root);
    }
    fs::create_dir_all(&recordings_root)
        .map_err(|e| format!("Failed to recreate recordings folder: {}", e))?;
    sqlx::query("VACUUM")
        .execute(state.db.pool())
        .await
        .map_err(|e| format!("Failed to compact database: {}", e))?;
    Ok(())
}
