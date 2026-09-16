use super::agent_feed::{AgentServices, PhonePromptSetting};
use super::agent_frames::{ClientFrame, ServerBody};
use super::agent_socket::Connection;
use crate::core::agent_bridge::{AgentBridge, SendError};
use crate::core::agent_sessions::{AgentApp, AgentRoots};
use crate::core::config::Config;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

const TRANSCRIPT: &str = r#"{"type":"user","cwd":"/tmp","message":{"content":"hide the header"},"uuid":"u-1","timestamp":"2026-09-14T00:04:00.100Z"}
{"type":"assistant","message":{"id":"m-1","stop_reason":"end_turn","content":[{"type":"text","text":"Done."}]},"uuid":"r-1","timestamp":"2026-09-14T00:04:02.000Z"}
"#;

struct Harness {
    connection: Connection,
    outbox: mpsc::Receiver<ServerBody>,
    config: Arc<Mutex<Config>>,
    transcript: std::path::PathBuf,
}

fn harness(name: &str) -> Harness {
    let base = std::env::temp_dir().join(format!("source_agent_socket_{name}"));
    let _ = std::fs::remove_dir_all(&base);
    let roots = AgentRoots {
        codex: base.join("codex"),
        claude: base.join("claude"),
        factory: base.join("factory"),
        opencode: base.join("opencode"),
    };
    let project = roots.claude.join("projects/-tmp");
    std::fs::create_dir_all(&project).unwrap();
    let transcript = project.join("s-1.jsonl");
    std::fs::write(&transcript, TRANSCRIPT).unwrap();

    let config = Arc::new(Mutex::new(Config::default()));
    // No Claude program: sends that get past the setting fail fast and visibly.
    let bridge = AgentBridge::with_binary(roots.clone(), None, Duration::from_secs(60));
    let (_tx, changes) = watch::channel(0u64);
    let (_setting, can_send_changes) = PhonePromptSetting::new(false);
    let agents = AgentServices { bridge, changes, can_send_changes, config: config.clone(), roots };
    let (out, outbox) = mpsc::channel(64);
    Harness { connection: Connection::new(agents, out), outbox, config, transcript }
}

async fn next(outbox: &mut mpsc::Receiver<ServerBody>) -> ServerBody {
    tokio::time::timeout(Duration::from_secs(5), outbox.recv()).await.expect("a frame").expect("open")
}

#[tokio::test]
async fn starts_with_a_snapshot_and_sending_turned_off() {
    let mut h = harness("snapshot");
    assert!(h.connection.send_snapshot().await);
    match next(&mut h.outbox).await {
        ServerBody::Snapshot { sessions, can_send, .. } => {
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].id, "s-1");
            assert!(!can_send, "a paired phone can't send prompts until it's turned on");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn refuses_to_send_until_the_setting_is_on() {
    let mut h = harness("toggle");
    let send = |id: &str| ClientFrame::SendText { request_id: id.into(), session_id: "s-1".into(), text: "hi".into() };

    assert!(h.connection.handle(send("r-off")).await);
    match next(&mut h.outbox).await {
        ServerBody::SendResult { request_id, ok, error: Some(SendError::Driver { message }) } => {
            assert_eq!(request_id, "r-off");
            assert!(!ok);
            assert!(message.contains("turned off"), "{message}");
        }
        other => panic!("{other:?}"),
    }

    h.config.lock().unwrap().mobile_agent_prompts_enabled = true;
    assert!(h.connection.handle(send("r-on")).await);
    match next(&mut h.outbox).await {
        // Past the setting, the bridge runs: here it reports that no Claude program exists.
        ServerBody::SendResult { request_id, ok, error } => {
            assert_eq!(request_id, "r-on");
            assert!(!ok);
            assert_eq!(error, Some(SendError::NoClaudeProgram));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn tells_the_phone_when_the_setting_is_switched() {
    let mut h = harness("can_send");
    h.config.lock().unwrap().mobile_agent_prompts_enabled = true;
    assert!(h.connection.send_can_send());
    assert_eq!(next(&mut h.outbox).await, ServerBody::CanSend { can_send: true });
}

#[test]
fn announces_only_real_changes() {
    let (setting, mut changes) = PhonePromptSetting::new(false);
    setting.announce(false);
    assert!(!changes.has_changed().unwrap(), "same value: phones aren't bothered");
    setting.announce(true);
    assert!(changes.has_changed().unwrap());
    assert!(*changes.borrow_and_update());
}

#[tokio::test]
async fn rejects_an_empty_prompt() {
    let mut h = harness("empty");
    h.config.lock().unwrap().mobile_agent_prompts_enabled = true;
    h.connection
        .handle(ClientFrame::SendText { request_id: "r".into(), session_id: "s-1".into(), text: "   ".into() })
        .await;
    assert!(matches!(next(&mut h.outbox).await, ServerBody::SendResult { ok: false, .. }));
}

#[tokio::test]
async fn follows_the_open_conversation_and_only_sends_changes() {
    let mut h = harness("follow");
    h.connection.handle(ClientFrame::OpenSession { app: AgentApp::ClaudeCode, session_id: "s-1".into() }).await;
    match next(&mut h.outbox).await {
        ServerBody::Messages { messages, .. } => assert_eq!(messages.len(), 2),
        other => panic!("{other:?}"),
    }

    // A change elsewhere: nothing new to send for this conversation.
    assert!(h.connection.refresh().await);
    let first_refresh = h.outbox.try_recv();
    assert!(matches!(first_refresh, Ok(ServerBody::Sessions { .. }) | Err(_)), "{first_refresh:?}");
    assert!(h.outbox.try_recv().is_err(), "unchanged messages aren't resent");

    // The conversation grows: its messages go out again.
    let mut grown = TRANSCRIPT.to_string();
    grown.push_str(r#"{"type":"user","message":{"content":"now the footer"},"uuid":"u-2","timestamp":"2026-09-14T00:05:00.000Z"}"#);
    grown.push('\n');
    std::fs::write(&h.transcript, grown).unwrap();
    assert!(h.connection.refresh().await);
    let mut saw_update = false;
    while let Ok(body) = h.outbox.try_recv() {
        if let ServerBody::Messages { messages, .. } = body {
            saw_update = messages.last().map(|m| m.text.as_str()) == Some("now the footer");
        }
    }
    assert!(saw_update);

    h.connection.handle(ClientFrame::CloseSession).await;
    assert!(h.connection.refresh().await);
    assert!(
        !std::iter::from_fn(|| h.outbox.try_recv().ok()).any(|b| matches!(b, ServerBody::Messages { .. })),
        "a closed conversation isn't followed"
    );
}
