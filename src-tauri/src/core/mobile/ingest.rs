use super::types::AuthDevice;
use super::wav::ensure_wav_header;
use crate::core::database::Database;
use crate::core::multimodal::{
    mark_mobile_clip_delivered, persist_mobile_transcript, track_mobile_clip, SupervisorCommand,
    MOBILE_SOURCE_ID,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Take an upload already streamed to `spooled`, move it into place, track it,
/// and dispatch transcription.
///
/// `clip_id` doubles as `asr_segment_id`, so a retried upload replaces its row
/// rather than duplicating it.
pub async fn ingest_clip_spooled(
    db: &Arc<Database>,
    commands: &Option<mpsc::Sender<SupervisorCommand>>,
    session_id: &str,
    device: &AuthDevice,
    clip_id: &str,
    started_at_ms: i64,
    ended_at_ms: i64,
    spooled: &Path,
    bytes_len: u64,
) -> Result<PathBuf, String> {
    let kind = ClipKind::detect(&read_header(spooled)?)?;
    let audio_path = stored_clip_path(clip_id, kind)?;

    // The phone re-sends anything it never saw acknowledged. If this clip is
    // already transcribed, accept it without redoing the work: a long
    // recording would otherwise re-run through Parakeet on every retry.
    if already_transcribed(db, clip_id).await {
        discard_spool(spooled);
        return Ok(audio_path);
    }

    if let Some(parent) = audio_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create mobile spool dir: {error}"))?;
    }
    std::fs::rename(spooled, &audio_path)
        .map_err(|error| format!("Failed to move mobile clip into place: {error}"))?;
    // A compressed upload replaces the WAV the live stream wrote for the same
    // recording. The clip's row is repointed at the new file just below.
    if kind != ClipKind::Wav {
        discard_spool(&mobile_clip_path(clip_id)?);
    }
    let path_str = audio_path.to_string_lossy().to_string();
    track_mobile_clip(
        db,
        clip_id,
        &device.device_id,
        started_at_ms,
        ended_at_ms,
        &path_str,
        bytes_len as i64,
    )
    .await?;
    // Placeholder before transcription, not after. In the other order a short
    // clip's transcript can land first and the placeholder then blanks it.
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
    dispatch_transcribe(commands, clip_id, &path_str, started_at_ms, ended_at_ms).await;
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

/// The audio formats the phone sends: WAV from older versions of the app, and
/// compressed AAC, either as a raw ADTS stream or in an MP4 container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Wav,
    Aac,
    M4a,
}

impl ClipKind {
    /// Recognise a clip by its first bytes rather than trusting its name.
    pub fn detect(header: &[u8]) -> Result<Self, String> {
        if header.len() < 12 {
            return Err("Uploaded clip is too small.".to_string());
        }
        if &header[0..4] == b"RIFF" && &header[8..12] == b"WAVE" {
            return if header.len() >= 44 {
                Ok(Self::Wav)
            } else {
                Err("Uploaded clip is too small.".to_string())
            };
        }
        if &header[4..8] == b"ftyp" {
            return Ok(Self::M4a);
        }
        // ADTS frames open with a 12-bit sync word followed by a zero layer.
        if header[0] == 0xFF && header[1] & 0xF6 == 0xF0 {
            return Ok(Self::Aac);
        }
        Err("Uploaded clip must be WAV, AAC or M4A audio.".to_string())
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Aac => "aac",
            Self::M4a => "m4a",
        }
    }
}

/// Where a received clip is kept, named for its format so Parakeet and the
/// timeline player both recognise it.
fn stored_clip_path(clip_id: &str, kind: ClipKind) -> Result<PathBuf, String> {
    Ok(mobile_clip_path(clip_id)?.with_extension(kind.extension()))
}

/// The live stream's spool file, and the name incoming uploads are spooled under.
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

/// Remove a spool file. A missing file is fine: it may already be moved.
pub fn discard_spool(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn read_header(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path)
        .map_err(|error| format!("Failed to read uploaded clip: {error}"))?;
    let mut header = Vec::with_capacity(44);
    file.take(44)
        .read_to_end(&mut header)
        .map_err(|error| format!("Failed to read uploaded clip: {error}"))?;
    Ok(header)
}

async fn already_transcribed(db: &Arc<Database>, clip_id: &str) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM asr_segments
         WHERE asr_segment_id = ? AND is_final = 1 AND length(trim(transcript)) > 0",
    )
    .bind(clip_id)
    .fetch_one(db.pool())
    .await
    .map(|count| count > 0)
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::ClipKind;

    #[test]
    fn recognises_each_format_the_phone_sends() {
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.resize(44, 0);
        assert_eq!(ClipKind::detect(&wav), Ok(ClipKind::Wav));
        assert_eq!(ClipKind::detect(b"\0\0\0\x20ftypM4A \0\0\0\0"), Ok(ClipKind::M4a));
        let adts = [0xFF, 0xF1, 0x60, 0x40, 0x2A, 0x3F, 0xFC, 0x21, 0, 0, 0, 0];
        assert_eq!(ClipKind::detect(&adts), Ok(ClipKind::Aac));
    }

    #[test]
    fn refuses_anything_else() {
        assert!(ClipKind::detect(b"not audio at all").is_err());
        assert!(ClipKind::detect(b"RIFF").is_err());
        let mut short_wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        short_wav.resize(20, 0);
        assert!(ClipKind::detect(&short_wav).is_err());
    }
}
