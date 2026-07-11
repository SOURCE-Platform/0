use super::audio_capture_support::{
    analyze_audio_chunk, classify_audio_trigger_reason, persist_sound_events,
    persist_speech_emotion, transcribe_audio_chunk,
};
use super::audio_intelligence_indexing::reindex_sound_event_spans;
use super::constants::{
    AUDIO_CHUNK_DURATION_MS, AUDIO_CHUNK_DURATION_SECS, EVIDENCE_AUDIT_INTERVAL_MS, WHISPER_VERSION,
};
use super::desktop_audio_runtime::capture_desktop_audio_chunk;
use super::indexing::reindex_audio_state_spans;
use super::media_io::{capture_audio_chunk, save_audio_evidence_chunk};
use crate::core::database::Database;
use crate::core::storage::RecordingStorage;
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(super) enum AudioCaptureSource {
    Microphone { audio_index: i32 },
    DesktopOutput { gain_db: f32 },
}

pub(super) async fn run_audio_loop(
    db: Arc<Database>,
    storage: Arc<RecordingStorage>,
    generation_ref: Arc<AtomicU64>,
    generation: u64,
    session_id: String,
    source_id: String,
    capture_source: AudioCaptureSource,
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

        if let Err(error) = capture_source
            .capture_chunk(AUDIO_CHUNK_DURATION_SECS, &temp_path)
            .await
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
                let asr_segment_id = Uuid::new_v4().to_string();
                let _ = sqlx::query(
                    "INSERT INTO asr_segments (
                        asr_segment_id, session_id, source_id, start_timestamp, end_timestamp,
                        language, transcript, confidence, model_name, model_version, audio_chunk_ids_json, created_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(asr_segment_id.clone())
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

                let _ = persist_speech_emotion(
                    &db,
                    &temp_path,
                    &session_id,
                    &source_id,
                    &audio_chunk_id,
                    Some(asr_segment_id),
                    capture_started,
                    capture_ended,
                )
                .await;
            }
        }

        let _ = persist_sound_events(
            &db,
            &temp_path,
            &session_id,
            &source_id,
            &audio_chunk_id,
            capture_started,
            capture_ended,
            speech_detected,
        )
        .await;

        let _ = reindex_audio_state_spans(&db, &session_id, &source_id).await;
        let _ = reindex_sound_event_spans(&db, &session_id, &source_id).await;
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

impl AudioCaptureSource {
    async fn capture_chunk(
        &self,
        duration_secs: f32,
        output_path: &std::path::Path,
    ) -> Result<(), String> {
        match self {
            Self::Microphone { audio_index } => {
                capture_audio_chunk(*audio_index, duration_secs, output_path).await
            }
            Self::DesktopOutput { gain_db } => {
                capture_desktop_audio_chunk(duration_secs, *gain_db, output_path).await
            }
        }
    }
}
