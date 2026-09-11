use super::audio_capture_support::{
    persist_sound_events, persist_speech_emotion, transcribe_audio_chunk,
};
use super::audio_intelligence_indexing::reindex_sound_event_spans;
use super::audio_transcripts::TranscriptAccumulator;
use super::foreground_coordinator::wait_for_background_transcription;
use super::indexing::reindex_audio_state_spans;
use super::types::AudioAnalysisOptions;
use crate::core::database::Database;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

static SPEECH_ANALYSIS_SLOT: OnceLock<Arc<Semaphore>> = OnceLock::new();
static SOUND_ANALYSIS_SLOT: OnceLock<Arc<Semaphore>> = OnceLock::new();
static EMOTION_ANALYSIS_SLOT: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn speech_analysis_slot() -> Arc<Semaphore> {
    SPEECH_ANALYSIS_SLOT
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

fn sound_analysis_slot() -> Arc<Semaphore> {
    SOUND_ANALYSIS_SLOT
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

fn emotion_analysis_slot() -> Arc<Semaphore> {
    EMOTION_ANALYSIS_SLOT
        .get_or_init(|| Arc::new(Semaphore::new(1)))
        .clone()
}

pub(super) fn try_reserve_speech_analysis() -> Option<OwnedSemaphorePermit> {
    speech_analysis_slot().try_acquire_owned().ok()
}

pub(super) fn try_reserve_sound_analysis() -> Option<OwnedSemaphorePermit> {
    sound_analysis_slot().try_acquire_owned().ok()
}

pub(super) fn spawn_speech_analysis_task(
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
    permit: OwnedSemaphorePermit,
) {
    tokio::spawn(async move {
        let speech_permit = permit;
        wait_for_background_transcription().await;
        let path = Path::new(&audio_path);
        let mut linked_asr_id = None;
        if analysis_options.transcription_enabled {
            match transcribe_audio_chunk(path).await {
                Some(transcription) => {
                    if let Some(utterance_id) = utterance_id.as_deref() {
                        let _ = transcripts
                            .append(utterance_id, start_timestamp, transcription)
                            .await;
                        linked_asr_id = Some(utterance_id.to_string());
                    }
                }
                None => {
                    eprintln!(
                        "ambient transcription returned no text (silent chunk or Parakeet failure): session {session_id} source {source_id} chunk {audio_chunk_id}"
                    );
                }
            }
        }
        drop(speech_permit);
        if analysis_options.speech_emotion_enabled {
            if let Ok(_permit) = emotion_analysis_slot().try_acquire_owned() {
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
        }
        let _ = reindex_audio_state_spans(&db, &session_id, &source_id).await;
        remove_audio_input(&db, &audio_chunk_id, &audio_path).await;
    });
}

pub(super) fn spawn_sound_analysis_task(
    db: Arc<Database>,
    session_id: String,
    source_id: String,
    audio_chunk_id: String,
    start_timestamp: i64,
    end_timestamp: i64,
    speech_detected: bool,
    audio_path: PathBuf,
    permit: OwnedSemaphorePermit,
) {
    tokio::spawn(async move {
        let _permit = permit;
        let _ = persist_sound_events(
            &db,
            &audio_path,
            &session_id,
            &source_id,
            &audio_chunk_id,
            start_timestamp,
            end_timestamp,
            speech_detected,
        )
        .await;
        let _ = reindex_sound_event_spans(&db, &session_id, &source_id).await;
        let _ = std::fs::remove_file(audio_path);
    });
}

pub(super) async fn cleanup_stale_audio_inputs(db: &Arc<Database>) {
    let cutoff = chrono::Utc::now().timestamp_millis() - 60_000;
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT audio_chunk_id, audio_path FROM audio_chunks
         WHERE retained_as_evidence = 1 AND audio_path IS NOT NULL AND created_at < ?",
    )
    .bind(cutoff)
    .fetch_all(db.pool())
    .await
    .unwrap_or_default();

    for (audio_chunk_id, audio_path) in rows {
        remove_audio_input(db, &audio_chunk_id, &audio_path).await;
    }
}

async fn remove_audio_input(db: &Arc<Database>, audio_chunk_id: &str, audio_path: &str) {
    let _ = std::fs::remove_file(audio_path);
    let _ = sqlx::query(
        "UPDATE audio_chunks SET audio_path = NULL, retained_as_evidence = 0
         WHERE audio_chunk_id = ?",
    )
    .bind(audio_chunk_id)
    .execute(db.pool())
    .await;
}
