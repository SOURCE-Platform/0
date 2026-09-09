use super::types::AuthDevice;
use crate::core::database::Database;
use crate::core::multimodal::{
    mark_mobile_clip_delivered, persist_mobile_transcript, track_mobile_clip, SupervisorCommand,
    MOBILE_SOURCE_ID,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Spool an uploaded WAV to disk, track it, and dispatch transcription.
/// `clip_id` doubles as `asr_segment_id` so retries replace, not duplicate.
pub async fn ingest_clip_file(
    db: &Arc<Database>,
    commands: &Option<mpsc::Sender<SupervisorCommand>>,
    session_id: &str,
    device: &AuthDevice,
    clip_id: &str,
    started_at_ms: i64,
    ended_at_ms: i64,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    validate_wav(bytes)?;
    let audio_path = mobile_clip_path(clip_id)?;
    if let Some(parent) = audio_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create mobile spool dir: {error}"))?;
    }
    std::fs::write(&audio_path, bytes)
        .map_err(|error| format!("Failed to spool mobile clip: {error}"))?;
    let path_str = audio_path.to_string_lossy().to_string();
    track_mobile_clip(
        db,
        clip_id,
        &device.device_id,
        started_at_ms,
        ended_at_ms,
        &path_str,
        bytes.len() as i64,
    )
    .await?;
    dispatch_transcribe(
        commands,
        clip_id,
        &path_str,
        started_at_ms,
        ended_at_ms,
    )
    .await;
    // Optimistic partial row so the lane shows a live block immediately.
    let _ = persist_mobile_transcript(
        db,
        session_id,
        clip_id,
        "",
        started_at_ms,
        ended_at_ms,
        None,
        None,
        "parakeet-tdt",
        false,
    )
    .await;
    Ok(audio_path)
}

/// Append streamed PCM (16 kHz mono Int16LE) to the spool file.
pub async fn append_stream_pcm(
    clip_id: &str,
    pcm: &[u8],
) -> Result<PathBuf, String> {
    let path = mobile_clip_path(clip_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create mobile spool dir: {error}"))?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("Failed to open stream spool: {error}"))?;
    file.write_all(pcm)
        .map_err(|error| format!("Failed to append stream PCM: {error}"))?;
    Ok(path)
}

pub async fn finish_stream_clip(
    db: &Arc<Database>,
    commands: &Option<mpsc::Sender<SupervisorCommand>>,
    session_id: &str,
    device: &AuthDevice,
    clip_id: &str,
    started_at_ms: i64,
    ended_at_ms: i64,
) -> Result<(), String> {
    let path = mobile_clip_path(clip_id)?;
    let path_str = path.to_string_lossy().to_string();
    let bytes = std::fs::metadata(&path).map(|meta| meta.len() as i64).unwrap_or(0);
    // Wrap raw PCM in a WAV header if needed (streamed path has no header).
    ensure_wav_header(&path)?;
    track_mobile_clip(
        db,
        clip_id,
        &device.device_id,
        started_at_ms,
        ended_at_ms,
        &path_str,
        bytes,
    )
    .await?;
    dispatch_transcribe(commands, clip_id, &path_str, started_at_ms, ended_at_ms).await;
    let _ = mark_mobile_clip_delivered(db, clip_id).await;
    let _ = session_id;
    Ok(())
}

async fn dispatch_transcribe(
    commands: &Option<mpsc::Sender<SupervisorCommand>>,
    clip_id: &str,
    path: &str,
    started_at_ms: i64,
    ended_at_ms: i64,
) {
    let Some(sender) = commands else {
        // Silence here used to look identical to success: the clip landed on
        // disk, the placeholder row stayed empty, and nothing said why.
        eprintln!(
            "Mobile clip {clip_id} stored but not transcribed: dictation helper is not running"
        );
        return;
    };
    if let Err(error) = sender
        .send(SupervisorCommand::TranscribeFile {
            id: clip_id.to_string(),
            path: path.to_string(),
            source: MOBILE_SOURCE_ID.to_string(),
            started_at_ms,
            ended_at_ms,
        })
        .await
    {
        eprintln!("Mobile clip {clip_id} could not be queued for transcription: {error}");
    } else {
        println!("Mobile clip {clip_id} queued for transcription");
    }
}

pub fn mobile_clip_path(clip_id: &str) -> Result<PathBuf, String> {
    let safe: String = clip_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return Err("Invalid clip id.".to_string());
    }
    let base = crate::platform::get_platform()
        .get_data_directory()
        .map_err(|_| "No data directory.".to_string())?;
    Ok(base.join("recordings").join("mobile").join(format!("{safe}.wav")))
}

fn validate_wav(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 44 {
        return Err("Uploaded clip is too small.".to_string());
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Uploaded clip must be a WAV file.".to_string());
    }
    Ok(())
}

fn ensure_wav_header(path: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Failed to read spool: {error}"))?;
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" {
        return Ok(());
    }
    // Raw Int16LE mono 16 kHz -> wrap with a WAV header.
    let header = wav_header(bytes.len() as u32);
    let mut out = Vec::with_capacity(header.len() + bytes.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&bytes);
    std::fs::write(path, out).map_err(|error| format!("Failed to wrap PCM: {error}"))?;
    Ok(())
}

fn wav_header(data_len: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36u32 + data_len).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&16000u32.to_le_bytes());
    header.extend_from_slice(&32000u32.to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_len.to_le_bytes());
    header
}
