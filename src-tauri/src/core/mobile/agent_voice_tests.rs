use super::agent_feed::{AgentServices, PhonePromptSetting};
use super::agent_frames::{ClientFrame, ServerBody, TalkState};
use super::agent_socket::Connection;
use super::agent_voice::VoiceServices;
use crate::core::agent_bridge::{AgentBridge, HandoffError, SendError};
use crate::core::agent_sessions::AgentRoots;
use crate::core::config::Config;
use crate::core::multimodal::transcript_waiters::transcript_waiters;
use crate::core::multimodal::SupervisorCommand;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

struct Harness {
    connection: Connection,
    outbox: mpsc::Receiver<ServerBody>,
    /// What the Mac asked the speech engine to transcribe.
    engine: mpsc::Receiver<SupervisorCommand>,
    config: Arc<Mutex<Config>>,
}

fn harness(name: &str) -> Harness {
    let base = std::env::temp_dir().join(format!("source_agent_voice_{name}"));
    let _ = std::fs::remove_dir_all(&base);
    let roots = AgentRoots {
        codex: base.join("codex"),
        claude: base.join("claude"),
        factory: base.join("factory"),
        opencode: base.join("opencode"),
    };
    let config = Arc::new(Mutex::new(Config::default()));
    config.lock().unwrap().mobile_agent_prompts_enabled = true;
    // No Claude program: a prompt that gets all the way through fails visibly at the end.
    let bridge = AgentBridge::with_binary(roots.clone(), None, Duration::from_secs(60));
    let (_changes_tx, changes) = watch::channel(0u64);
    let (_setting, can_send_changes) = PhonePromptSetting::new(true);
    let agents = AgentServices { bridge, changes, can_send_changes, config: config.clone(), roots };
    let (engine_tx, engine) = mpsc::channel(8);
    let voice = VoiceServices {
        commands: Arc::new(tokio::sync::Mutex::new(Some(engine_tx))),
        db: None,
        session: None,
        audio_dir: Some(base.join("audio")),
    };
    let (out, outbox) = mpsc::channel(64);
    Harness { connection: Connection::new(agents, Some(voice), out), outbox, engine, config }
}

async fn next(outbox: &mut mpsc::Receiver<ServerBody>) -> ServerBody {
    tokio::time::timeout(Duration::from_secs(5), outbox.recv()).await.expect("a frame").expect("open")
}

async fn next_talk(outbox: &mut mpsc::Receiver<ServerBody>) -> TalkState {
    match next(outbox).await {
        ServerBody::Talk { state, .. } => state,
        other => panic!("expected a talk frame, got {other:?}"),
    }
}

/// Hold for `seconds` of silence-shaped audio, in phone-sized chunks, then release.
async fn talk(h: &mut Harness, talk_id: &str, seconds: f64) {
    let start = ClientFrame::TalkStart { talk_id: talk_id.into(), session_id: "s-1".into() };
    assert!(h.connection.handle(start).await);
    let total = (seconds * 32_000.0) as usize;
    for _ in 0..total / 3_200 {
        h.connection.audio(&[0u8; 3_200]);
    }
    assert!(h.connection.handle(ClientFrame::TalkEnd { talk_id: talk_id.into() }).await);
}

/// Play the speech engine: take the transcription request, answer it with `text`.
async fn transcribe_as(h: &mut Harness, text: &str) -> PathBuf {
    let command = tokio::time::timeout(Duration::from_secs(5), h.engine.recv()).await.unwrap().unwrap();
    let SupervisorCommand::TranscribeFile { id, path, source, .. } = command else { panic!("not a transcription") };
    assert_eq!(source, crate::core::multimodal::MOBILE_SOURCE_ID, "lands in the timeline as phone speech");
    transcript_waiters().complete(&id, text);
    PathBuf::from(path)
}

#[tokio::test]
async fn a_spoken_prompt_is_transcribed_confirmed_and_sent() {
    let mut h = harness("happy");
    talk(&mut h, "voice-happy", 3.0).await;
    assert_eq!(next_talk(&mut h.outbox).await, TalkState::Transcribing { delayed: false });

    let wav = transcribe_as(&mut h, "  hide the header ").await;
    assert_eq!(std::fs::metadata(&wav).unwrap().len(), 44 + 96_000, "3 s of audio saved as WAV");
    assert_eq!(
        next_talk(&mut h.outbox).await,
        TalkState::Confirm { text: "hide the header".into(), send_in_ms: 3_000 }
    );

    h.connection.handle(ClientFrame::SendNow { talk_id: "voice-happy".into() }).await;
    assert_eq!(next_talk(&mut h.outbox).await, TalkState::Sending { text: "hide the header".into() });
    match next(&mut h.outbox).await {
        ServerBody::SendResult { request_id, error, .. } => {
            assert_eq!(request_id, "voice-happy", "the send result carries the talk id");
            // It reached the bridge, which finds no such conversation in this sandbox.
            assert_eq!(error, Some(SendError::Handoff(HandoffError::NoTranscript)));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn cancelling_in_the_confirm_window_sends_nothing() {
    let mut h = harness("cancel_send");
    talk(&mut h, "voice-cancel-send", 1.0).await;
    next_talk(&mut h.outbox).await;
    transcribe_as(&mut h, "delete everything").await;
    assert!(matches!(next_talk(&mut h.outbox).await, TalkState::Confirm { .. }));

    h.connection.handle(ClientFrame::CancelSend { talk_id: "voice-cancel-send".into() }).await;
    assert_eq!(next_talk(&mut h.outbox).await, TalkState::Cancelled);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(h.outbox.try_recv().is_err(), "no send result follows");
}

#[tokio::test]
async fn a_tap_or_silence_is_no_speech() {
    let mut h = harness("no_speech");
    talk(&mut h, "voice-tap", 0.1).await;
    assert_eq!(next_talk(&mut h.outbox).await, TalkState::NoSpeech, "too short to transcribe");
    assert!(h.engine.try_recv().is_err());

    talk(&mut h, "voice-silence", 1.0).await;
    next_talk(&mut h.outbox).await;
    transcribe_as(&mut h, "").await;
    assert_eq!(next_talk(&mut h.outbox).await, TalkState::NoSpeech);
}

#[tokio::test]
async fn sliding_away_while_talking_drops_the_audio() {
    let mut h = harness("talk_cancel");
    let start = ClientFrame::TalkStart { talk_id: "voice-slide".into(), session_id: "s-1".into() };
    h.connection.handle(start).await;
    h.connection.audio(&[0u8; 32_000]);
    h.connection.handle(ClientFrame::TalkCancel { talk_id: "voice-slide".into() }).await;
    assert_eq!(next_talk(&mut h.outbox).await, TalkState::Cancelled);

    h.connection.handle(ClientFrame::TalkEnd { talk_id: "voice-slide".into() }).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(h.engine.try_recv().is_err(), "nothing was transcribed");
}

#[tokio::test]
async fn voice_prompts_are_refused_while_the_setting_is_off() {
    let mut h = harness("off");
    h.config.lock().unwrap().mobile_agent_prompts_enabled = false;
    let start = ClientFrame::TalkStart { talk_id: "voice-off".into(), session_id: "s-1".into() };
    h.connection.handle(start).await;
    match next_talk(&mut h.outbox).await {
        TalkState::Failed { message } => assert!(message.contains("turned off"), "{message}"),
        other => panic!("{other:?}"),
    }
}
