//! Push-to-talk on `/v1/agent`.
//!
//! The phone streams a voice prompt's audio while the button is held. On
//! release the Mac saves it, transcribes it with the same speech engine as
//! everything else (so it also lands in the timeline as a phone transcript),
//! shows the phone what it heard with a short window to cancel, then sends it
//! into the conversation exactly like a typed prompt.

use super::agent_feed::AgentServices;
use super::agent_frames::{ServerBody, TalkState};
use super::agent_send_window::{resolve, Decision, CONFIRM_WINDOW};
use super::agent_socket::deliver_prompt;
use super::ingest::mobile_clip_path;
use super::wav::{write_wav, BYTES_PER_SECOND};
use crate::core::database::Database;
use crate::core::multimodal::foreground_coordinator::is_background_transcription_paused;
use crate::core::multimodal::transcript_waiters::transcript_waiters;
use crate::core::multimodal::{persist_mobile_transcript, SupervisorCommand, MOBILE_SOURCE_ID};
use crate::core::session_manager::SessionManager;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

/// Shorter than this is a tap, not speech.
const MIN_AUDIO: usize = BYTES_PER_SECOND * 3 / 10;
/// Five minutes. Audio past this is dropped rather than growing without limit.
const MAX_AUDIO: usize = BYTES_PER_SECOND * 300;

/// What voice prompts need beyond the agent services: the speech engine, and
/// the timeline they're recorded in. Tests leave the timeline out and keep the
/// audio in a folder of their own.
#[derive(Clone)]
pub struct VoiceServices {
    pub commands: Arc<Mutex<Option<mpsc::Sender<SupervisorCommand>>>>,
    pub db: Option<Arc<Database>>,
    pub session: Option<Arc<SessionManager>>,
    /// Where the audio goes; `None` for phone recordings' usual folder.
    pub audio_dir: Option<std::path::PathBuf>,
}

struct Recording {
    talk_id: String,
    session_id: String,
    audio: Vec<u8>,
    started_at_ms: i64,
}

/// One connection's voice prompts: the one being recorded, and the confirm
/// windows still open for earlier ones.
#[derive(Default)]
pub(super) struct Talks {
    recording: Option<Recording>,
    windows: HashMap<String, mpsc::Sender<Decision>>,
}

impl Talks {
    pub(super) fn start(&mut self, talk_id: String, session_id: String) {
        self.windows.retain(|_, window| !window.is_closed());
        let started_at_ms = chrono::Utc::now().timestamp_millis();
        self.recording = Some(Recording { talk_id, session_id, audio: Vec::new(), started_at_ms });
    }

    pub(super) fn audio(&mut self, bytes: &[u8]) {
        if let Some(recording) = &mut self.recording {
            let room = MAX_AUDIO.saturating_sub(recording.audio.len());
            recording.audio.extend_from_slice(&bytes[..bytes.len().min(room)]);
        }
    }

    /// Stop recording `talk_id`, returning its conversation if it was the one.
    pub(super) fn cancel(&mut self, talk_id: &str) -> Option<String> {
        let recording = self.recording.take_if(|recording| recording.talk_id == talk_id)?;
        Some(recording.session_id)
    }

    pub(super) fn decide(&mut self, talk_id: &str, decision: Decision) {
        if let Some(window) = self.windows.get(talk_id) {
            let _ = window.try_send(decision);
        }
    }

    /// Released: hand the audio to its own task, which reports every step.
    pub(super) fn end(&mut self, talk_id: &str, agents: &AgentServices, voice: &VoiceServices, out: &mpsc::Sender<ServerBody>) {
        let Some(recording) = self.recording.take_if(|recording| recording.talk_id == talk_id) else {
            return;
        };
        let (window, decisions) = mpsc::channel(4);
        self.windows.insert(recording.talk_id.clone(), window);
        let utterance = Utterance { agents: agents.clone(), voice: voice.clone(), out: out.clone(), recording };
        tokio::spawn(utterance.run(decisions));
    }
}

struct Utterance {
    agents: AgentServices,
    voice: VoiceServices,
    out: mpsc::Sender<ServerBody>,
    recording: Recording,
}

impl Utterance {
    fn push(&self, state: TalkState) {
        let _ = self.out.try_send(ServerBody::Talk {
            talk_id: self.recording.talk_id.clone(),
            session_id: self.recording.session_id.clone(),
            state,
        });
    }

    fn fail(&self, message: &str) {
        self.push(TalkState::Failed { message: message.to_string() });
    }

    async fn run(self, decisions: mpsc::Receiver<Decision>) {
        let Some(text) = self.transcribe().await else { return };
        let send_in_ms = CONFIRM_WINDOW.as_millis() as u64;
        self.push(TalkState::Confirm { text: text.clone(), send_in_ms });
        if !resolve(CONFIRM_WINDOW, decisions).await {
            self.push(TalkState::Cancelled);
            return;
        }
        self.push(TalkState::Sending { text: text.clone() });
        let recording = &self.recording;
        deliver_prompt(&self.agents, &self.out, recording.talk_id.clone(), recording.session_id.clone(), text).await;
    }

    /// The words, or `None` once the phone has been told why there are none.
    async fn transcribe(&self) -> Option<String> {
        let audio = &self.recording.audio;
        if audio.len() < MIN_AUDIO {
            self.push(TalkState::NoSpeech);
            return None;
        }
        let safe_id: String = self.recording.talk_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        let clip_id = format!("agent-{safe_id}");
        let path = match &self.voice.audio_dir {
            Some(dir) if !safe_id.is_empty() => Ok(dir.join(format!("{clip_id}.wav"))),
            _ if safe_id.is_empty() => Err(()),
            _ => mobile_clip_path(&clip_id).map_err(|_| ()),
        };
        let Ok(path) = path else {
            self.fail("That voice prompt had an invalid id.");
            return None;
        };
        if let Err(error) = write_wav(&path, audio) {
            self.fail(&error);
            return None;
        }
        let started_at_ms = self.recording.started_at_ms;
        let ended_at_ms = started_at_ms + (audio.len() * 1000 / BYTES_PER_SECOND) as i64;
        self.record_placeholder(&clip_id, started_at_ms, ended_at_ms).await;

        // Registered before dispatch, so a very quick result can't be missed.
        let waiter = transcript_waiters().register(&clip_id);
        let delayed = is_background_transcription_paused();
        self.push(TalkState::Transcribing { delayed });
        let Some(commands) = self.voice.commands.lock().await.clone() else {
            transcript_waiters().forget(&clip_id);
            self.fail("Speech recognition isn't running on the Mac yet. Try again in a moment.");
            return None;
        };
        let command = SupervisorCommand::TranscribeFile {
            id: clip_id.clone(),
            path: path.to_string_lossy().to_string(),
            source: MOBILE_SOURCE_ID.to_string(),
            started_at_ms,
            ended_at_ms,
        };
        if commands.send(command).await.is_err() {
            transcript_waiters().forget(&clip_id);
            self.fail("Speech recognition on the Mac stopped. Try again in a moment.");
            return None;
        }

        // Dictation on the Mac holds the engine; allow for waiting behind it.
        let base = Duration::from_secs(if delayed { 120 } else { 30 });
        let limit = base + Duration::from_millis(2 * (ended_at_ms - started_at_ms).max(0) as u64);
        let text = match tokio::time::timeout(limit, waiter).await {
            Ok(Ok(text)) => text,
            Ok(Err(_)) => {
                self.fail("Speech recognition on the Mac restarted. Try again.");
                return None;
            }
            Err(_) => {
                transcript_waiters().forget(&clip_id);
                self.fail("Transcribing took too long. Try again.");
                return None;
            }
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            self.push(TalkState::NoSpeech);
            return None;
        }
        Some(text)
    }

    /// An empty timeline row first, filled when the transcript lands, as phone
    /// clips do. In the other order a quick transcript could be blanked.
    async fn record_placeholder(&self, clip_id: &str, started_at_ms: i64, ended_at_ms: i64) {
        let Some(db) = &self.voice.db else { return };
        let session_id = match &self.voice.session {
            Some(manager) => match manager.get_current_session().await {
                Ok(Some(session)) => session.id,
                _ => "mobile".to_string(),
            },
            None => "mobile".to_string(),
        };
        let _ = persist_mobile_transcript(
            db, &session_id, clip_id, "", started_at_ms, ended_at_ms, None, None, "parakeet-tdt", false,
        )
        .await;
    }
}
