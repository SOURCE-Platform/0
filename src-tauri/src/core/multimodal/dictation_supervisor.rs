use super::dictation_helper::{DictationEvent, DictationHelper};
use super::dictation_pipeline::{DictationPipeline, PipelineAction};
use super::foreground_coordinator::{capture_timestamp_ms, set_background_transcription_paused};
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
    /// Returns the action + event receivers plus a command sender for
    /// driving the helper (insert text, stop sessions, shut down).
    pub async fn run(
        mut self,
    ) -> Result<
        (
            mpsc::Receiver<PipelineAction>,
            broadcast::Receiver<DictationEvent>,
            mpsc::Sender<SupervisorCommand>,
        ),
        String,
    > {
        let (helper, mut events) = DictationHelper::spawn().await?;
        let event_feed = helper.subscribe();
        let (actions_tx, actions_rx) = mpsc::channel(64);
        let (commands_tx, mut commands_rx) = mpsc::channel::<SupervisorCommand>(64);
        tokio::spawn(async move {
            self.helper = Some(helper);
            loop {
                tokio::select! {
                    incoming = events.recv() => {
                        let event = match incoming {
                            Ok(event) => event,
                            Err(_) => break,
                        };
                        update_background_priority(&event);
                        log_helper_event(&event);
                        let done = matches!(event, DictationEvent::Exited);
                        let action = self
                            .pipeline
                            .on_event(&event, &self.dictionary, capture_timestamp_ms());
                        Self::forward(action, &mut self.pipeline, &actions_tx).await;
                        if done {
                            break;
                        }
                    }
                    command = commands_rx.recv() => {
                        let Some(command) = command else { break };
                        if !Self::execute(command, &mut self.helper).await {
                            break;
                        }
                    }
                }
            }
        });
        Ok((actions_rx, event_feed, commands_tx))
    }

    async fn forward(
        action: PipelineAction,
        pipeline: &mut DictationPipeline,
        actions_tx: &mpsc::Sender<PipelineAction>,
    ) {
        // Insertion follows persistence: emit the follow-up action
        // right after PersistForeground so typing never precedes save.
        match action {
            PipelineAction::PersistForeground {
                id,
                text,
                started_at_ms,
                ended_at_ms,
                language,
                confidence,
                model,
            } => {
                let insert = pipeline.take_pending_insertion(&text);
                let _ = actions_tx
                    .send(PipelineAction::PersistForeground {
                        id,
                        text: text.clone(),
                        started_at_ms,
                        ended_at_ms,
                        language,
                        confidence,
                        model,
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
    }

    /// Returns false when the loop should exit.
    async fn execute(command: SupervisorCommand, helper: &mut Option<DictationHelper>) -> bool {
        let Some(helper) = helper else {
            return true;
        };
        let result = match command {
            SupervisorCommand::StartSession(id) => helper.start_session(&id).await,
            SupervisorCommand::StopSession => helper.stop_session().await,
            SupervisorCommand::Insert(id, text) => helper.request_insertion(&id, &text).await,
            SupervisorCommand::Shutdown => return false,
        };
        if let Err(error) = result {
            eprintln!("Dictation command failed: {error}");
        }
        true
    }
}

fn update_background_priority(event: &DictationEvent) {
    match event {
        DictationEvent::SessionStarted { .. } => set_background_transcription_paused(true),
        DictationEvent::Transcript(_)
        | DictationEvent::EngineError(_)
        | DictationEvent::Exited
        | DictationEvent::Ready => set_background_transcription_paused(false),
        _ => {}
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
        DictationEvent::OpenSettings => {
            println!("Dictation settings requested from pill");
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
