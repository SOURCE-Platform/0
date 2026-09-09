use super::dictation_helper::{DictationEvent, DictationTranscript};
use super::dictation_store::DICTATION_SOURCE_ID;
use super::foreground_coordinator::ForegroundCoordinator;
use super::speech_provider::{apply_dictionary_entries, DictionaryEntry};
use std::collections::HashSet;

/// Routes helper events into timeline actions. Pure logic — the service
/// layer performs the DB writes and helper commands this decides on.
pub struct DictationPipeline {
    coordinator: ForegroundCoordinator,
    seen_transcript_ids: HashSet<String>,
    pending_insertion: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineAction {
    /// Transcript is new: persist with dictionary applied.
    PersistForeground {
        id: String,
        text: String,
        started_at_ms: i64,
        ended_at_ms: i64,
        language: Option<String>,
        confidence: Option<f32>,
        model: String,
    },
    /// Transcript text ready to type into the focused field.
    InsertIntoFocusedField { id: String, text: String },
    /// Gear clicked on the pill: open O's dictation settings.
    OpenSettings,
    /// Duplicate delivery: acknowledge, do not persist twice.
    DuplicateIgnored { id: String },
    None,
}

impl DictationPipeline {
    pub fn new() -> Self {
        Self {
            coordinator: ForegroundCoordinator::new(),
            seen_transcript_ids: HashSet::new(),
            pending_insertion: None,
        }
    }

    pub fn on_event(
        &mut self,
        event: &DictationEvent,
        dictionary: &[DictionaryEntry],
        now_ms: i64,
    ) -> PipelineAction {
        match event {
            DictationEvent::SessionStarted { id } => {
                self.coordinator.on_session_started(id, now_ms);
                self.pending_insertion = None;
                PipelineAction::None
            }
            DictationEvent::SessionStopped { id } => {
                self.coordinator.on_session_stopped(id);
                PipelineAction::None
            }
            DictationEvent::Transcript(transcript) => {
                self.on_transcript(transcript, dictionary)
            }
            DictationEvent::OpenSettings => PipelineAction::OpenSettings,
            DictationEvent::Inserted { .. }
            | DictationEvent::Debug(_)
            | DictationEvent::Ready
            | DictationEvent::Exited => PipelineAction::None,
            DictationEvent::EngineError(_) => PipelineAction::None,
        }
    }

    /// Background chunks ask here first: false means buffer, not transcribe.
    pub fn admit_background_chunk(&mut self) -> bool {
        self.coordinator.admit_background_chunk()
    }

    pub fn buffered_background_chunks(&self) -> u64 {
        self.coordinator.buffered_background_chunks()
    }

    pub fn drain_buffered(&mut self) -> u64 {
        self.coordinator.drain_buffered()
    }

    fn on_transcript(
        &mut self,
        transcript: &DictationTranscript,
        dictionary: &[DictionaryEntry],
    ) -> PipelineAction {
        if !self.seen_transcript_ids.insert(transcript.id.clone()) {
            return PipelineAction::DuplicateIgnored {
                id: transcript.id.clone(),
            };
        }
        let text = apply_dictionary_entries(&transcript.text, dictionary);
        if text.trim().is_empty() {
            return PipelineAction::DuplicateIgnored {
                id: transcript.id.clone(),
            };
        }
        // Only Right Option dictation types into the focused field.
        // Remote sources (e.g. Source Mobile) persist to the timeline
        // but must never drive insertion.
        if transcript.source == DICTATION_SOURCE_ID {
            self.pending_insertion = Some(transcript.id.clone());
        } else {
            self.pending_insertion = None;
        }
        // Persist first; the caller issues insertion next so a typing
        // failure never loses the captured audio/text.
        PipelineAction::PersistForeground {
            id: transcript.id.clone(),
            text: text.clone(),
            started_at_ms: transcript.started_at_ms,
            ended_at_ms: transcript.ended_at_ms,
            language: transcript.language.clone(),
            confidence: transcript.confidence,
            model: transcript.model.clone(),
        }
    }

    pub fn take_pending_insertion(&mut self, text: &str) -> Option<PipelineAction> {
        self.pending_insertion
            .take()
            .map(|id| PipelineAction::InsertIntoFocusedField {
                id,
                text: text.to_string(),
            })
    }
}

impl Default for DictationPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(id: &str, text: &str) -> DictationTranscript {
        DictationTranscript {
            id: id.to_string(),
            text: text.to_string(),
            language: None,
            confidence: None,
            provider: "native-helper".to_string(),
            model: "parakeet-tdt-v3".to_string(),
            started_at_ms: 1000,
            ended_at_ms: 2000,
            source: "fluid-voice-prompt".to_string(),
            is_final: true,
        }
    }

    #[test]
    fn session_buffers_background_and_persists_transcript() {
        let mut pipeline = DictationPipeline::new();
        pipeline.on_event(&DictationEvent::SessionStarted { id: "s1".to_string() }, &[], 1);
        assert!(!pipeline.admit_background_chunk());
        match pipeline.on_event(
            &DictationEvent::Transcript(transcript("t1", "hello")),
            &[],
            2,
        ) {
            PipelineAction::PersistForeground { id, text, .. } => {
                assert_eq!(id, "t1");
                assert_eq!(text, "hello");
            }
            other => panic!("unexpected: {other:?}"),
        }
        pipeline.on_event(&DictationEvent::SessionStopped { id: "s1".to_string() }, &[], 3);
        assert!(pipeline.admit_background_chunk());
        assert_eq!(pipeline.drain_buffered(), 1);
    }

    #[test]
    fn duplicate_transcript_persisted_once() {
        let mut pipeline = DictationPipeline::new();
        let first = pipeline.on_event(
            &DictationEvent::Transcript(transcript("t1", "hello")),
            &[],
            1,
        );
        assert!(matches!(first, PipelineAction::PersistForeground { .. }));
        match pipeline.on_event(&DictationEvent::Transcript(transcript("t1", "hello")), &[], 2) {
            PipelineAction::DuplicateIgnored { id } => assert_eq!(id, "t1"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn dictionary_applied_before_persist() {
        let mut pipeline = DictationPipeline::new();
        let dictionary = vec![DictionaryEntry {
            triggers: vec!["cub rick".to_string()],
            replacement: "Kubrick".to_string(),
        }];
        match pipeline.on_event(
            &DictationEvent::Transcript(transcript("t1", "met cub rick")),
            &dictionary,
            1,
        ) {
            PipelineAction::PersistForeground { text, .. } => {
                assert_eq!(text, "met Kubrick")
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn empty_after_dictionary_is_ignored() {
        let mut pipeline = DictationPipeline::new();
        match pipeline.on_event(
            &DictationEvent::Transcript(transcript("t1", "   ")),
            &[],
            1,
        ) {
            PipelineAction::DuplicateIgnored { .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn remote_source_persists_without_insertion() {
        let mut pipeline = DictationPipeline::new();
        let mut remote = transcript("mobile-1", "hello from phone");
        remote.source = "source-mobile".to_string();
        match pipeline.on_event(&DictationEvent::Transcript(remote), &[], 1) {
            PipelineAction::PersistForeground { id, .. } => assert_eq!(id, "mobile-1"),
            other => panic!("unexpected: {other:?}"),
        }
        assert!(pipeline.take_pending_insertion("hello from phone").is_none());
    }
}
