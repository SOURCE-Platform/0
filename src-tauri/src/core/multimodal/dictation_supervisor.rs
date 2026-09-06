use super::dictation_helper::{DictationEvent, DictationHelper};
use super::dictation_pipeline::{DictationPipeline, PipelineAction};
use super::foreground_coordinator::capture_timestamp_ms;
use super::speech_provider::DictionaryEntry;
use tokio::sync::{broadcast, mpsc};

/// Owns the helper process and its pipeline. The service layer creates
/// one supervisor, forwards its action channel into timeline writes and
/// insertion commands, and never touches the helper directly.
pub struct DictationSupervisor {
    pipeline: DictationPipeline,
    dictionary: Vec<DictionaryEntry>,
    helper: Option<DictationHelper>,
}

/// Commands into the running helper. Wired in Phase C (live loop).
#[allow(dead_code)]
pub enum SupervisorCommand {
    StartSession(String),
    StopSession,
    Insert(String, String),
    Shutdown,
}

impl DictationSupervisor {
    pub fn new(dictionary: Vec<DictionaryEntry>) -> Self {
        Self {
            pipeline: DictationPipeline::new(),
            dictionary,
            helper: None,
        }
    }

    pub fn set_dictionary(&mut self, dictionary: Vec<DictionaryEntry>) {
        self.dictionary = dictionary;
    }

    /// Spawn the helper and pump its events into pipeline actions.
    /// Returns receivers for actions and raw helper events.
    pub async fn run(
        mut self,
    ) -> Result<
        (
            mpsc::Receiver<PipelineAction>,
            broadcast::Receiver<DictationEvent>,
        ),
        String,
    > {
        let (helper, mut events) = DictationHelper::spawn().await?;
        let event_feed = helper.subscribe();
        let (actions_tx, actions_rx) = mpsc::channel(64);
        tokio::spawn(async move {
            self.helper = Some(helper);
            loop {
                let event = match events.recv().await {
                    Ok(event) => event,
                    Err(_) => break,
                };
                log_helper_event(&event);
                let done = matches!(event, DictationEvent::Exited);
                let action = self
                    .pipeline
                    .on_event(&event, &self.dictionary, capture_timestamp_ms());
                // Insertion follows persistence: emit the follow-up action
                // right after PersistForeground so typing never precedes save.
                match action {
                    PipelineAction::PersistForeground {
                        id,
                        text,
                        started_at_ms,
                        ended_at_ms,
                    } => {
                        let insert = self.pipeline.take_pending_insertion(&text);
                        let _ = actions_tx
                            .send(PipelineAction::PersistForeground {
                                id,
                                text: text.clone(),
                                started_at_ms,
                                ended_at_ms,
                            })
                            .await;
                        if let Some(insert) = insert {
                            debug_assert!(matches!(
                                insert,
                                PipelineAction::InsertIntoFocusedField { .. }
                            ));
                            let _ = actions_tx.send(insert).await;
                        }
                    }
                    PipelineAction::None => {}
                    other => {
                        let _ = actions_tx.send(other).await;
                    }
                }
                if done {
                    break;
                }
            }
        });
        Ok((actions_rx, event_feed))
    }
}

/// Every helper line lands here, so Right Option presses are visible
/// in the host log even before transcription or typing is involved.
fn log_helper_event(event: &DictationEvent) {
    match event {
        DictationEvent::Ready => {}
        DictationEvent::SessionStarted { id } => {
            println!("Dictation session started ({id})");
        }
        DictationEvent::SessionStopped { id } => {
            println!("Dictation session stopped ({id})");
        }
        DictationEvent::Transcript(transcript) => {
            println!(
                "Dictation transcript received ({} chars)",
                transcript.text.len()
            );
        }
        DictationEvent::Inserted { id } => {
            println!("Dictation {id} typed into focused field");
        }
        DictationEvent::Debug(message) => {
            println!("Dictation debug: {message}");
        }
        DictationEvent::EngineError(message) => {
            eprintln!("Dictation helper error: {message}");
        }
        DictationEvent::Exited => {}
    }
}
#[cfg(test)]
mod tests {
    use super::super::dictation_helper::DictationTranscript;
    use super::*;

    fn transcript(id: &str, text: &str) -> DictationTranscript {
        DictationTranscript {
            id: id.to_string(),
            text: text.to_string(),
            language: None,
            confidence: None,
            provider: "native-helper".to_string(),
            model: "parakeet-tdt-v3".to_string(),
            started_at_ms: 1,
            ended_at_ms: 2,
            source: "fluid-voice-prompt".to_string(),
            is_final: true,
        }
    }

    #[test]
    fn supervisor_applies_dictionary_through_pipeline() {
        let dictionary = vec![DictionaryEntry {
            triggers: vec!["cub rick".to_string()],
            replacement: "Kubrick".to_string(),
        }];
        let mut supervisor = DictationSupervisor::new(dictionary);
        // Drive the inner pipeline directly: same path `run()` uses.
        let action = supervisor.pipeline.on_event(
            &DictationEvent::Transcript(transcript("t1", "hi cub rick")),
            &supervisor.dictionary,
            1,
        );
        match action {
            PipelineAction::PersistForeground { text, .. } => {
                assert_eq!(text, "hi Kubrick")
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}
