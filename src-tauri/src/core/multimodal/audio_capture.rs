use super::audio_capture_support::{
    analyze_audio_chunk, classify_audio_trigger_reason, persist_sound_events,
    persist_speech_emotion, transcribe_audio_chunk,
};
use super::audio_intelligence_indexing::reindex_sound_event_spans;
use super::audio_transcripts::TranscriptAccumulator;
use super::constants::{
    AUDIO_CHUNK_DURATION_MS, AUDIO_CHUNK_DURATION_SECS, EVIDENCE_AUDIT_INTERVAL_MS,
};
use super::desktop_audio_runtime::capture_desktop_audio_chunk;
use super::indexing::reindex_audio_state_spans;
use super::media_io::{capture_audio_chunk, save_audio_evidence_chunk};
use super::types::AudioAnalysisOptions;
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
    analysis_options: AudioAnalysisOptions,
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
    let transcripts = TranscriptAccumulator::new(db.clone(), session_id.clone(), source_id.clone());

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
        let analysis = match analyze_audio_chunk(&temp_path) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("audio VAD failed: {error}");
                let _ = fs::remove_file(&temp_path);
                sleep(Duration::from_millis(AUDIO_CHUNK_DURATION_MS as u64)).await;
                continue;
            }
        };

        let trigger_reason = classify_audio_trigger_reason(
            previous_speech_detected,
            analysis.speech_detected,
            analysis.vad_score,
        );
        let retain_evidence = analysis.speech_detected
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
                audio_path, retained_as_evidence, vad_score, speech_detected, waveform_json, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&audio_chunk_id)
        .bind(&session_id)
        .bind(&source_id)
        .bind(capture_started)
        .bind(capture_ended)
        .bind(trigger_reason)
        .bind(retained_path.clone())
        .bind(if retained_path.is_some() { 1 } else { 0 })
        .bind(analysis.vad_score as f64)
        .bind(if analysis.speech_detected { 1 } else { 0 })
        .bind(json!(analysis.waveform_levels).to_string())
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(db.pool())
        .await;

        let active_utterance_id =
            if analysis.speech_detected && analysis_options.transcription_enabled {
                Some(
                    transcripts
                        .begin_chunk(audio_chunk_id.clone(), capture_started, capture_ended)
                        .await,
                )
            } else if analysis_options.transcription_enabled {
                let _ = transcripts.finalize_active().await;
                None
            } else {
                None
            };

        if analysis.speech_detected
            && (analysis_options.transcription_enabled || analysis_options.speech_emotion_enabled)
        {
            if let Some(transcript_source) = retained_path.clone() {
                spawn_speech_analysis_task(
                    db.clone(),
                    session_id.clone(),
                    source_id.clone(),
                    audio_chunk_id.clone(),
                    capture_started,
                    capture_ended,
                    transcript_source,
                    transcripts.clone(),
                    active_utterance_id,
                    analysis_options,
                );
            }
        }

        if analysis_options.sound_events_enabled {
            let _ = persist_sound_events(
                &db,
                &temp_path,
                &session_id,
                &source_id,
                &audio_chunk_id,
                capture_started,
                capture_ended,
                analysis.speech_detected,
            )
            .await;
        }

        let _ = reindex_audio_state_spans(&db, &session_id, &source_id).await;
        let _ = reindex_sound_event_spans(&db, &session_id, &source_id).await;
        previous_speech_detected = analysis.speech_detected;
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

    if analysis_options.transcription_enabled {
        let _ = transcripts.finalize_active().await;
    }
}

fn spawn_speech_analysis_task(
    db: Arc<Database>,
    session_id: String,
    source_id: String,
    audio_chunk_id: String,
    start_timestamp: i64,
    end_timestamp: i64,
    audio_path: String,
    transcripts: TranscriptAccumulator,
    utterance_id: Option<String>,
    analysis_options: AudioAnalysisOptions,
) {
    tokio::spawn(async move {
        let path = std::path::Path::new(&audio_path);
        let mut linked_asr_id = None;
        if analysis_options.transcription_enabled {
            let Some(transcription) = transcribe_audio_chunk(path).await else {
                return;
            };
            if let Some(utterance_id) = utterance_id.as_deref() {
                let _ = transcripts
                    .append(utterance_id, start_timestamp, transcription)
                    .await;
                linked_asr_id = Some(utterance_id.to_string());
            }
        }
        if analysis_options.speech_emotion_enabled {
            let _ = persist_speech_emotion(
                &db,
                path,
                &session_id,
                &source_id,
                &audio_chunk_id,
                linked_asr_id,
                start_timestamp,
                end_timestamp,
            )
            .await;
        }
        let _ = reindex_audio_state_spans(&db, &session_id, &source_id).await;
    });
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
