use super::audio_runtime::ParakeetTranscription;
use super::constants::{PARAKEET_MODEL, PARAKEET_VERSION};
use crate::core::database::Database;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub(super) struct TranscriptAccumulator {
    db: Arc<Database>,
    session_id: String,
    source_id: String,
    state: Arc<Mutex<TranscriptState>>,
}

#[derive(Default)]
struct TranscriptState {
    active_utterance_id: Option<String>,
    utterances: HashMap<String, TranscriptUtterance>,
}

struct TranscriptUtterance {
    start_timestamp: i64,
    end_timestamp: i64,
    audio_chunk_ids: Vec<String>,
    parts: BTreeMap<i64, TranscriptPart>,
    is_final: bool,
}

struct TranscriptPart {
    text: String,
    language: Option<String>,
    confidence: Option<f32>,
}

struct TranscriptWrite {
    utterance_id: String,
    start_timestamp: i64,
    end_timestamp: i64,
    transcript: String,
    language: Option<String>,
    confidence: Option<f32>,
    audio_chunk_ids: Vec<String>,
    is_final: bool,
}

impl TranscriptAccumulator {
    pub(super) fn new(db: Arc<Database>, session_id: String, source_id: String) -> Self {
        Self {
            db,
            session_id,
            source_id,
            state: Arc::new(Mutex::new(TranscriptState::default())),
        }
    }

    pub(super) async fn begin_chunk(
        &self,
        audio_chunk_id: String,
        start_timestamp: i64,
        end_timestamp: i64,
    ) -> String {
        let mut state = self.state.lock().await;
        let utterance_id = state.active_utterance_id.clone().unwrap_or_else(|| {
            let new_id = Uuid::new_v4().to_string();
            state.active_utterance_id = Some(new_id.clone());
            state.utterances.insert(
                new_id.clone(),
                TranscriptUtterance {
                    start_timestamp,
                    end_timestamp,
                    audio_chunk_ids: Vec::new(),
                    parts: BTreeMap::new(),
                    is_final: false,
                },
            );
            new_id
        });

        if let Some(utterance) = state.utterances.get_mut(&utterance_id) {
            utterance.end_timestamp = utterance.end_timestamp.max(end_timestamp);
            utterance.audio_chunk_ids.push(audio_chunk_id);
        }
        utterance_id
    }

    pub(super) async fn append(
        &self,
        utterance_id: &str,
        start_timestamp: i64,
        transcription: ParakeetTranscription,
    ) -> Result<(), String> {
        let write = {
            let mut state = self.state.lock().await;
            let utterance = state
                .utterances
                .get_mut(utterance_id)
                .ok_or_else(|| "Audio transcript utterance was no longer available.".to_string())?;
            utterance.parts.insert(
                start_timestamp,
                TranscriptPart {
                    text: transcription.text,
                    language: transcription.language,
                    confidence: transcription.confidence,
                },
            );
            build_write(utterance_id, utterance)
        };
        self.persist(write).await
    }

    pub(super) async fn finalize_active(&self) -> Result<(), String> {
        let write = {
            let mut state = self.state.lock().await;
            let Some(utterance_id) = state.active_utterance_id.take() else {
                return Ok(());
            };
            let Some(utterance) = state.utterances.get_mut(&utterance_id) else {
                return Ok(());
            };
            utterance.is_final = true;
            (!utterance.parts.is_empty()).then(|| build_write(&utterance_id, utterance))
        };
        if let Some(write) = write {
            self.persist(write).await?;
        }
        Ok(())
    }

    async fn persist(&self, write: TranscriptWrite) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO asr_segments (
                asr_segment_id, session_id, source_id, start_timestamp, end_timestamp,
                language, transcript, confidence, model_name, model_version, audio_chunk_ids_json,
                created_at, is_final
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(asr_segment_id) DO UPDATE SET
                end_timestamp = excluded.end_timestamp,
                language = excluded.language,
                transcript = excluded.transcript,
                confidence = excluded.confidence,
                audio_chunk_ids_json = excluded.audio_chunk_ids_json,
                created_at = excluded.created_at,
                is_final = excluded.is_final",
        )
        .bind(write.utterance_id)
        .bind(&self.session_id)
        .bind(&self.source_id)
        .bind(write.start_timestamp)
        .bind(write.end_timestamp)
        .bind(write.language)
        .bind(write.transcript)
        .bind(write.confidence.map(f64::from))
        .bind(PARAKEET_MODEL)
        .bind(PARAKEET_VERSION)
        .bind(json!(write.audio_chunk_ids).to_string())
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(i64::from(write.is_final))
        .execute(self.db.pool())
        .await
        .map_err(|error| format!("Failed to persist live transcript: {error}"))?;
        Ok(())
    }
}

fn build_write(utterance_id: &str, utterance: &TranscriptUtterance) -> TranscriptWrite {
    let transcript = utterance
        .parts
        .values()
        .map(|part| part.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let confidences = utterance
        .parts
        .values()
        .filter_map(|part| part.confidence)
        .collect::<Vec<_>>();
    TranscriptWrite {
        utterance_id: utterance_id.to_string(),
        start_timestamp: utterance.start_timestamp,
        end_timestamp: utterance.end_timestamp,
        transcript,
        language: utterance
            .parts
            .values()
            .find_map(|part| part.language.clone()),
        confidence: (!confidences.is_empty())
            .then(|| confidences.iter().sum::<f32>() / confidences.len() as f32),
        audio_chunk_ids: utterance.audio_chunk_ids.clone(),
        is_final: utterance.is_final,
    }
}
