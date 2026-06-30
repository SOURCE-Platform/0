use super::constants::{
    AUDIO_CHUNK_DURATION_MS, AUDIO_CHUNK_DURATION_SECS, AUDIO_VAD_WINDOW_MS,
    EVIDENCE_AUDIT_INTERVAL_MS, VAD_RMS_INTERMITTENT_THRESHOLD, VAD_RMS_SPEECH_THRESHOLD,
    WHISPER_MODEL, WHISPER_VERSION,
};
use super::indexing::reindex_audio_state_spans;
use super::media_io::{capture_audio_chunk, save_audio_evidence_chunk};
use super::service::command_available;
use crate::core::database::Database;
use crate::core::storage::RecordingStorage;
use hound::WavReader;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::process::Command;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

pub(super) async fn run_audio_loop(
    db: Arc<Database>,
    storage: Arc<RecordingStorage>,
    generation_ref: Arc<AtomicU64>,
    generation: u64,
    session_id: String,
    source_id: String,
    audio_index: i32,
) {
    let session_uuid = match Uuid::parse_str(&session_id) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("audio loop could not parse session id: {error}");
            return;
        }
    };
    let mut last_audit_evidence_at: Option<i64> = None;
    let mut previous_speech_detected = false;

    while generation_ref.load(Ordering::SeqCst) == generation {
        let started_at = chrono::Utc::now().timestamp_millis();
        let temp_dir = std::env::temp_dir().join("source_audio_samples");
        let _ = fs::create_dir_all(&temp_dir);
        let temp_path = temp_dir.join(format!("audio-{}.wav", Uuid::new_v4()));
        let capture_started = chrono::Utc::now().timestamp_millis();

        if let Err(error) =
            capture_audio_chunk(audio_index, AUDIO_CHUNK_DURATION_SECS, &temp_path).await
        {
            eprintln!("audio capture failed: {error}");
            sleep(Duration::from_millis(AUDIO_CHUNK_DURATION_MS as u64)).await;
            continue;
        }

        let capture_ended = chrono::Utc::now().timestamp_millis();
        let (vad_score, speech_detected) = match analyze_audio_chunk(&temp_path) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("audio VAD failed: {error}");
                let _ = fs::remove_file(&temp_path);
                sleep(Duration::from_millis(AUDIO_CHUNK_DURATION_MS as u64)).await;
                continue;
            }
        };

        let trigger_reason =
            classify_audio_trigger_reason(previous_speech_detected, speech_detected, vad_score);
        let retain_evidence = speech_detected
            || last_audit_evidence_at
                .map(|last| capture_ended - last >= EVIDENCE_AUDIT_INTERVAL_MS)
                .unwrap_or(true);
        let retained_path = if retain_evidence {
            save_audio_evidence_chunk(&storage, session_uuid, &temp_path)
                .await
                .ok()
        } else {
            None
        };

        let audio_chunk_id = Uuid::new_v4().to_string();
        let _ = sqlx::query(
            "INSERT INTO audio_chunks (
                audio_chunk_id, session_id, source_id, start_timestamp, end_timestamp, trigger_reason,
                audio_path, retained_as_evidence, vad_score, speech_detected, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&audio_chunk_id)
        .bind(&session_id)
        .bind(&source_id)
        .bind(capture_started)
        .bind(capture_ended)
        .bind(trigger_reason)
        .bind(retained_path.clone())
        .bind(if retained_path.is_some() { 1 } else { 0 })
        .bind(vad_score as f64)
        .bind(if speech_detected { 1 } else { 0 })
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(db.pool())
        .await;

        if speech_detected {
            let transcript_source = retained_path
                .clone()
                .unwrap_or_else(|| temp_path.to_string_lossy().to_string());
            if let Some(transcript) = transcribe_audio_chunk(&transcript_source).await {
                let _ = sqlx::query(
                    "INSERT INTO asr_segments (
                        asr_segment_id, session_id, source_id, start_timestamp, end_timestamp,
                        language, transcript, confidence, model_name, model_version, audio_chunk_ids_json, created_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&session_id)
                .bind(&source_id)
                .bind(capture_started)
                .bind(capture_ended)
                .bind("en")
                .bind(transcript)
                .bind(0.72f64)
                .bind("whisper")
                .bind(WHISPER_VERSION)
                .bind(json!([audio_chunk_id.clone()]).to_string())
                .bind(chrono::Utc::now().timestamp_millis())
                .execute(db.pool())
                .await;
            }
        }

        let _ = reindex_audio_state_spans(&db, &session_id, &source_id).await;
        previous_speech_detected = speech_detected;
        if retain_evidence {
            last_audit_evidence_at = Some(capture_ended);
        }

        let _ = fs::remove_file(&temp_path);
        let elapsed = chrono::Utc::now().timestamp_millis() - started_at;
        if elapsed < AUDIO_CHUNK_DURATION_MS {
            sleep(Duration::from_millis(
                (AUDIO_CHUNK_DURATION_MS - elapsed) as u64,
            ))
            .await;
        }
    }
}

fn classify_audio_trigger_reason(
    previous_speech_detected: bool,
    speech_detected: bool,
    vad_score: f32,
) -> &'static str {
    if speech_detected && !previous_speech_detected {
        "speech_started"
    } else if !speech_detected && previous_speech_detected {
        "speech_stopped"
    } else if vad_score > 0.0 {
        "static_fallback"
    } else {
        "periodic_sample"
    }
}

fn analyze_audio_chunk(path: &Path) -> Result<(f32, bool), String> {
    let mut reader = WavReader::open(path).map_err(|e| format!("Failed to open WAV chunk: {e}"))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate as usize;
    let window_samples = (sample_rate * AUDIO_VAD_WINDOW_MS) / 1000;

    let mut window = Vec::with_capacity(window_samples.max(1));
    let mut max_rms = 0.0f32;
    let mut speech_windows = 0usize;
    let mut total_windows = 0usize;

    for sample in reader.samples::<i16>() {
        let sample = sample.map_err(|e| format!("Failed to read WAV sample: {e}"))?;
        window.push(sample as f32 / i16::MAX as f32);
        if window.len() >= window_samples.max(1) {
            let rms = rms_window(&window);
            max_rms = max_rms.max(rms);
            if rms >= VAD_RMS_SPEECH_THRESHOLD {
                speech_windows += 1;
            }
            total_windows += 1;
            window.clear();
        }
    }
    if !window.is_empty() {
        let rms = rms_window(&window);
        max_rms = max_rms.max(rms);
        if rms >= VAD_RMS_SPEECH_THRESHOLD {
            speech_windows += 1;
        }
        total_windows += 1;
    }

    let speech_detected = speech_windows >= 2
        || (total_windows > 0 && max_rms >= VAD_RMS_INTERMITTENT_THRESHOLD && speech_windows > 0);
    Ok((max_rms, speech_detected))
}

fn rms_window(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}

async fn transcribe_audio_chunk(path: &str) -> Option<String> {
    if !command_available("whisper").await {
        return None;
    }

    let output_dir = std::env::temp_dir().join(format!("source_whisper_{}", Uuid::new_v4()));
    if fs::create_dir_all(&output_dir).is_err() {
        return None;
    }

    let output = Command::new("whisper")
        .args([
            "--model",
            WHISPER_MODEL,
            "--output_dir",
            output_dir.to_string_lossy().as_ref(),
            "--output_format",
            "json",
            "--language",
            "en",
            path,
        ])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stem = Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("chunk");
    let json_path = output_dir.join(format!("{stem}.json"));
    let transcript = fs::read_to_string(json_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|json| {
            json.get("text")
                .and_then(|value| value.as_str())
                .map(|value| value.trim().to_string())
        })
        .filter(|text| !text.is_empty());
    let _ = fs::remove_dir_all(output_dir);
    transcript
}
