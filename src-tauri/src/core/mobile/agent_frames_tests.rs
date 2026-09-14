use super::agent_frames::{parse_client_frame, ClientFrame, ServerBody, ServerFrame};
use crate::core::agent_bridge::{DriverEvent, HandoffError, SendError};
use crate::core::agent_sessions::{AgentApp, AgentMessage, AgentSession, MessageRole};

const GOLDEN_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/core/mobile/fixtures/agent_frames.json");

fn session() -> AgentSession {
    AgentSession {
        id: "s-1".into(),
        app: AgentApp::ClaudeCode,
        title: "Marketing site header".into(),
        project_path: "/Users/a/site".into(),
        project_name: "site".into(),
        updated_at_ms: 1789344240100,
        preview: "The header now hides on scroll down.".into(),
        live: true,
        archived: false,
    }
}

/// One of every frame the Mac sends, in a fixed order.
fn every_server_frame() -> Vec<ServerFrame> {
    let bodies = vec![
        ServerBody::Snapshot { epoch: "e-1".into(), sessions: vec![session()], held: vec!["s-1".into()], can_send: true },
        ServerBody::Sessions { sessions: vec![session()], held: vec![] },
        ServerBody::Messages {
            session_id: "s-1".into(),
            messages: vec![AgentMessage { id: "u-1".into(), role: MessageRole::User, text: "hide the header".into(), at_ms: 1 }],
        },
        ServerBody::Turn { session_id: "s-1".into(), event: DriverEvent::Working, brief: None, still_working: false },
        ServerBody::Turn {
            session_id: "s-1".into(),
            event: DriverEvent::TurnDone { text: "Done.".into(), summary: Some("hid the header".into()), is_error: false, cost_usd: 0.01, duration_ms: 2500 },
            brief: Some("Done.".into()),
            still_working: false,
        },
        ServerBody::SendResult { request_id: "r-1".into(), ok: true, error: None },
        ServerBody::SendResult { request_id: "r-2".into(), ok: false, error: Some(SendError::Handoff(HandoffError::BusyInApp)) },
        ServerBody::Error { message: "Sending from the phone is turned off on the Mac.".into() },
        ServerBody::Ping,
    ];
    bodies.into_iter().enumerate().map(|(i, body)| ServerFrame { seq: i as u64 + 1, body }).collect()
}

#[test]
fn server_frames_match_the_golden_file_the_phone_also_reads() {
    let actual: Vec<serde_json::Value> =
        every_server_frame().iter().map(|frame| serde_json::from_str(&frame.to_json()).unwrap()).collect();
    let rendered = serde_json::to_string_pretty(&serde_json::json!({ "server": actual, "client": client_samples() })).unwrap();
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(GOLDEN_PATH, format!("{rendered}\n")).unwrap();
    }
    let golden: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(GOLDEN_PATH).unwrap()).unwrap();
    assert_eq!(golden["server"], serde_json::Value::Array(actual), "run with UPDATE_GOLDEN=1 after an intended change");
}

fn client_samples() -> serde_json::Value {
    serde_json::json!([
        { "type": "hello" },
        { "type": "open_session", "app": "claude-code", "sessionId": "s-1" },
        { "type": "close_session" },
        { "type": "send_text", "requestId": "r-1", "sessionId": "s-1", "text": "hide the header" },
        { "type": "pong" }
    ])
}

#[test]
fn parses_every_client_frame_in_the_golden_file() {
    let golden: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(GOLDEN_PATH).unwrap()).unwrap();
    let parsed: Vec<ClientFrame> = golden["client"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| parse_client_frame(&frame.to_string()).expect("known frame"))
        .collect();
    assert_eq!(
        parsed,
        vec![
            ClientFrame::Hello,
            ClientFrame::OpenSession { app: AgentApp::ClaudeCode, session_id: "s-1".into() },
            ClientFrame::CloseSession,
            ClientFrame::SendText { request_id: "r-1".into(), session_id: "s-1".into(), text: "hide the header".into() },
            ClientFrame::Pong,
        ]
    );
}

#[test]
fn ignores_unknown_or_broken_client_frames() {
    assert!(parse_client_frame(r#"{"type":"format_disk"}"#).is_none());
    assert!(parse_client_frame("not json").is_none());
    assert!(parse_client_frame(r#"{"type":"send_text","session_id":"s-1"}"#).is_none(), "missing fields");
}
