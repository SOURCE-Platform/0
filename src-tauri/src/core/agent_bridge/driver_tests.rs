use super::claude_driver::{ClaudeDriver, DriverConfig, DriverError};
use super::driver_events::{DriverEvent, TurnState};
use super::stream_protocol::{StreamEvent, TurnResult};
use std::path::PathBuf;
use std::time::Duration;

/// A stand-in for `claude`: replies to every message with a real captured turn.
fn fake_claude(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("source_fake_claude_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/src/core/agent_bridge/fixtures/claude_stream.jsonl");
    let script = dir.join("claude");
    std::fs::write(&script, format!("#!/bin/sh\nwhile IFS= read -r line; do cat '{fixture}'; done\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn config(binary: PathBuf, quiet: Duration) -> DriverConfig {
    DriverConfig {
        binary,
        session_id: "s-1".to_string(),
        cwd: std::env::temp_dir(),
        permission_mode: None,
        release_after_quiet: quiet,
    }
}

async fn next_matching(
    events: &mut tokio::sync::broadcast::Receiver<DriverEvent>,
    wanted: impl Fn(&DriverEvent) -> bool,
) -> Vec<DriverEvent> {
    let mut seen = Vec::new();
    let found = tokio::time::timeout(Duration::from_secs(5), async {
        while let Ok(event) = events.recv().await {
            let done = wanted(&event);
            seen.push(event);
            if done {
                return;
            }
        }
    })
    .await;
    assert!(found.is_ok(), "timed out; saw {seen:?}");
    seen
}

#[tokio::test]
async fn sends_a_message_and_reports_the_finished_turn() {
    let driver = ClaudeDriver::new(config(fake_claude("turn"), Duration::from_secs(60)));
    let mut events = driver.subscribe();

    assert_eq!(driver.send("reply DONE").await.unwrap(), DriverEvent::Working);
    assert!(driver.is_busy());
    assert!(driver.pid().is_some());

    let seen = next_matching(&mut events, |e| matches!(e, DriverEvent::TurnDone { .. })).await;
    assert_eq!(seen.first(), Some(&DriverEvent::Working));
    assert!(seen.contains(&DriverEvent::ToolUse { name: "Bash".into() }));
    assert!(seen.contains(&DriverEvent::Progress { detail: "Running echo hi".into() }));
    match seen.last().unwrap() {
        DriverEvent::TurnDone { text, summary, is_error, .. } => {
            assert_eq!(text, "DONE");
            assert_eq!(summary.as_deref(), Some("ran echo hi; replied DONE"));
            assert!(!is_error);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(!driver.is_busy());
    driver.release().await;
}

#[tokio::test]
async fn releases_on_request_and_starts_again_on_the_next_message() {
    let driver = ClaudeDriver::new(config(fake_claude("release"), Duration::from_secs(60)));
    let mut events = driver.subscribe();
    driver.send("first").await.unwrap();
    next_matching(&mut events, |e| matches!(e, DriverEvent::TurnDone { .. })).await;
    let first_pid = driver.pid();

    driver.release().await;
    next_matching(&mut events, |e| *e == DriverEvent::Released).await;
    assert!(driver.pid().is_none());

    driver.send("second").await.unwrap();
    next_matching(&mut events, |e| matches!(e, DriverEvent::TurnDone { .. })).await;
    assert!(driver.pid().is_some() && driver.pid() != first_pid);
    driver.release().await;
}

#[tokio::test]
async fn hands_the_conversation_back_after_a_quiet_period() {
    let driver = ClaudeDriver::new(config(fake_claude("quiet"), Duration::from_millis(300)));
    let mut events = driver.subscribe();
    driver.send("hello").await.unwrap();
    let seen = next_matching(&mut events, |e| *e == DriverEvent::Released).await;
    assert!(seen.iter().any(|e| matches!(e, DriverEvent::TurnDone { .. })), "released only after the turn");
    assert!(driver.pid().is_none());
}

#[tokio::test]
async fn reports_a_missing_claude_program() {
    let driver = ClaudeDriver::new(config(PathBuf::from("/nonexistent/claude"), Duration::from_secs(60)));
    assert!(matches!(driver.send("hi").await, Err(DriverError::Spawn(_))));
}

fn result(text: &str, queued: i64) -> StreamEvent {
    StreamEvent::TurnDone(TurnResult {
        is_error: false,
        text: text.to_string(),
        cost_usd: 0.0,
        duration_ms: 1,
        api_error_status: None,
        queued_turn_count: queued,
    })
}

#[test]
fn a_message_sent_mid_turn_joins_the_work_and_keeps_claude_busy() {
    let mut turn = TurnState::default();
    assert_eq!(turn.on_sent(), DriverEvent::Working);
    assert_eq!(turn.on_sent(), DriverEvent::AddedToCurrentWork);
    // The first turn ends with the follow-up still queued: still busy.
    turn.on_stream(result("first", 1));
    assert!(turn.busy());
    turn.on_stream(result("second", 0));
    assert!(!turn.busy());
}

#[test]
fn an_expired_sign_in_ends_the_turn_with_a_clear_event() {
    let mut turn = TurnState::default();
    turn.on_sent();
    let event = turn.on_stream(result("Failed to authenticate. API Error: 401 OAuth access token has expired.", 0));
    assert_eq!(event, Some(DriverEvent::AuthExpired));
    assert!(!turn.busy());
}

/// Real run: creates a throwaway conversation with the real `claude`, then
/// continues it through the driver.
/// `cargo test --lib driver_tests::live_continue_real_conversation -- --ignored --nocapture`
#[tokio::test]
#[ignore = "runs the real claude program and uses plan usage"]
async fn live_continue_real_conversation() {
    let binary = super::claude_cli::resolve_claude_binary().expect("claude installed");
    let cwd = std::env::temp_dir().join("source_driver_live");
    std::fs::create_dir_all(&cwd).unwrap();
    let created = tokio::process::Command::new(&binary)
        .args(["-p", "--model", "haiku", "--output-format", "json", "Remember the word LANTERN. Reply OK."])
        .current_dir(&cwd)
        .output()
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    let session_id = created["session_id"].as_str().unwrap().to_string();
    println!("created {session_id}: {}", created["result"]);

    let driver = ClaudeDriver::new(DriverConfig {
        binary,
        session_id,
        cwd,
        permission_mode: None,
        release_after_quiet: Duration::from_secs(60),
    });
    let mut events = driver.subscribe();
    let started = std::time::Instant::now();
    driver.send("What word did I ask you to remember? Answer with just the word.").await.unwrap();
    let seen = next_matching_long(&mut events, |e| matches!(e, DriverEvent::TurnDone { .. } | DriverEvent::AuthExpired)).await;
    println!("{:?} after {:?}", seen.last().unwrap(), started.elapsed());
    match seen.last().unwrap() {
        DriverEvent::TurnDone { text, .. } => assert!(text.to_uppercase().contains("LANTERN"), "{text}"),
        other => panic!("{other:?}"),
    }
    driver.release().await;
    assert!(driver.pid().is_none());
}

async fn next_matching_long(
    events: &mut tokio::sync::broadcast::Receiver<DriverEvent>,
    wanted: impl Fn(&DriverEvent) -> bool,
) -> Vec<DriverEvent> {
    let mut seen = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(120), async {
        while let Ok(event) = events.recv().await {
            let done = wanted(&event);
            seen.push(event);
            if done {
                return;
            }
        }
    })
    .await;
    seen
}
