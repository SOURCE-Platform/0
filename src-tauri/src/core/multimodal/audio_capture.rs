use super::audio_analysis_tasks::{
    cleanup_stale_audio_inputs, spawn_sound_analysis_task, spawn_speech_analysis_task,
    try_reserve_sound_analysis, try_reserve_speech_analysis,
};
use super::audio_capture_support::{analyze_audio_chunk, classify_audio_trigger_reason};
use super::audio_transcripts::TranscriptAccumulator;
use super::constants::{AUDIO_CHUNK_DURATION_MS, AUDIO_CHUNK_DURATION_SECS};
use super::desktop_audio_runtime::capture_desktop_audio_chunk;
use super::foreground_coordinator::{
    background_capture_epoch, background_transcription_is_paused, wait_for_background_transcription,
};
use super::indexing::reindex_audio_state_spans;
use super::media_io::save_audio_evidence_chunk;
use super::mic_capture::capture_microphone_chunk;
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
    Microphone { source_name: String },
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
    let mut previous_speech_detected = false;
    let transcripts = TranscriptAccumulator::new(db.clone(), session_id.clone(), source_id.clone());
    cleanup_stale_audio_inputs(&db).await;
    let mut last_cleanup_at = chrono::Utc::now().timestamp_millis();

    while generation_ref.load(Ordering::SeqCst) == generation {
        wait_for_background_transcription().await;
        if generation_ref.load(Ordering::SeqCst) != generation {
            break;
        }
        let capture_epoch = background_capture_epoch();
        let started_at = chrono::Utc::now().timestamp_millis();
        if started_at - last_cleanup_at >= 60_000 {
            cleanup_stale_audio_inputs(&db).await;
            last_cleanup_at = started_at;
        }
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
        // If Right Option took ownership at any point during this sample,
        // discard it. This prevents the same speech appearing in both lanes.
        if background_transcription_is_paused() || background_capture_epoch() != capture_epoch {
            let _ = fs::remove_file(&temp_path);
            previous_speech_detected = false;
            continue;
        }
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
        // Transcripts-only retention: raw audio exists solely as
        // transcription input. Evidence is kept only for chunks a speech
        // task will consume, and deleted the moment it is done (see the
        // cleanup at the end of the spawned task). Waveforms, transcripts,
        // and sound labels — all derived, none replayable — are what persist.
        let speech_permit = (analysis.speech_detected
            && (analysis_options.transcription_enabled || analysis_options.speech_emotion_enabled))
            .then(try_reserve_speech_analysis)
            .flatten();
        let retain_evidence = speech_permit.is_some();
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
            if speech_permit.is_some() && analysis_options.transcription_enabled {
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

        if let (Some(permit), Some(transcript_source)) = (speech_permit, retained_path.clone()) {
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
                permit,
            );
        }

        let sound_task_started = analysis_options.sound_events_enabled
            && try_reserve_sound_analysis().is_some_and(|permit| {
                spawn_sound_analysis_task(
                    db.clone(),
                    session_id.clone(),
                    source_id.clone(),
                    audio_chunk_id.clone(),
                    capture_started,
                    capture_ended,
                    analysis.speech_detected,
                    temp_path.clone(),
                    permit,
                );
                true
            });

        let _ = reindex_audio_state_spans(&db, &session_id, &source_id).await;
        previous_speech_detected = analysis.speech_detected;

        if !sound_task_started {
            let _ = fs::remove_file(&temp_path);
        }
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

impl AudioCaptureSource {
    async fn capture_chunk(
        &self,
        duration_secs: f32,
        output_path: &std::path::Path,
    ) -> Result<(), String> {
        match self {
            Self::Microphone { source_name } => {
                capture_microphone_chunk(source_name, duration_secs, output_path).await
            }
            Self::DesktopOutput { gain_db } => {
                capture_desktop_audio_chunk(duration_secs, *gain_db, output_path).await
            }
        }
    }
}
