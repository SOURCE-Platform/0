use super::handoff::{decide, take_over, terminate_and_wait, Decision, HandoffError};
use crate::core::agent_sessions::{turn_activity, AgentRoots, LiveClaudeSession, TurnActivity};
use std::time::Duration;

fn process(pid: i32, entrypoint: &str) -> LiveClaudeSession {
    LiveClaudeSession {
        pid,
        session_id: "s-1".into(),
        name: "site-a1".into(),
        cwd: "/Users/a/site".into(),
        entrypoint: entrypoint.into(),
    }
}

const IDLE: &str = r#"{"type":"user","message":{"content":"hi"}}
{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"Hello"}]}}
{"type":"system","subtype":"turn_duration"}"#;
const MID_TOOL: &str = r#"{"type":"user","message":{"content":"run tests"}}
{"type":"assistant","message":{"stop_reason":"tool_use","content":[{"type":"tool_use","name":"Bash"}]}}"#;
const AWAITING_REPLY: &str = r#"{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"Done"}]}}
{"type":"user","message":{"content":"now the footer"}}
{"type":"user","isMeta":true,"message":{"content":"<system-reminder>x</system-reminder>"}}"#;
const QUEUED: &str = r#"{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"Done"}]}}
{"type":"queue-operation","operation":"enqueue","content":"and the footer"}"#;
const QUEUE_DRAINED: &str = r#"{"type":"queue-operation","operation":"enqueue","content":"x"}
{"type":"queue-operation","operation":"dequeue"}
{"type":"user","message":{"content":"x"}}
{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text","text":"ok"}]}}"#;

#[test]
fn reads_whether_a_conversation_is_mid_turn() {
    assert_eq!(turn_activity(IDLE), TurnActivity::Idle);
    assert_eq!(turn_activity(MID_TOOL), TurnActivity::Busy);
    assert_eq!(turn_activity(AWAITING_REPLY), TurnActivity::Busy);
    assert_eq!(turn_activity(QUEUED), TurnActivity::Busy);
    assert_eq!(turn_activity(QUEUE_DRAINED), TurnActivity::Idle);
    assert_eq!(turn_activity(""), TurnActivity::Idle);
}

#[test]
fn decides_when_a_conversation_can_be_taken_over() {
    assert_eq!(decide(&[], &[], TurnActivity::Idle), Decision::Free);
    assert_eq!(decide(&[process(10, "claude-desktop")], &[10], TurnActivity::Busy), Decision::Free, "SOURCE's own process never blocks");
    assert_eq!(
        decide(&[process(10, "claude-desktop")], &[], TurnActivity::Idle),
        Decision::StopAppProcesses(vec![10])
    );
    assert_eq!(
        decide(&[process(10, "claude-desktop")], &[], TurnActivity::Busy),
        Decision::Refuse(HandoffError::BusyInApp)
    );
    assert_eq!(
        decide(&[process(10, "cli")], &[], TurnActivity::Idle),
        Decision::Refuse(HandoffError::RunningElsewhere { entrypoint: "cli".into() })
    );
}

#[test]
fn stops_a_process_and_waits_for_it_without_polling() {
    let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    assert!(terminate_and_wait(child.id() as i32, Duration::from_secs(5)));
    assert!(!child.wait().unwrap().success());
}

#[test]
fn reports_a_process_that_ignores_the_stop_request() {
    let mut stubborn = std::process::Command::new("sh")
        .args(["-c", "trap '' TERM; sleep 30"])
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200)); // let the shell install its trap
    assert!(!terminate_and_wait(stubborn.id() as i32, Duration::from_millis(500)));
    stubborn.kill().unwrap();
    let _ = stubborn.wait();
}

/// A fake "Claude app process" (a `sleep`) registered for a conversation.
fn fake_app_session(name: &str, transcript: &str, entrypoint: &str) -> (AgentRoots, std::process::Child) {
    let base = std::env::temp_dir().join(format!("source_handoff_{name}"));
    let _ = std::fs::remove_dir_all(&base);
    let roots = AgentRoots {
        codex: base.join("codex"),
        claude: base.join("claude"),
        factory: base.join("factory"),
        opencode: base.join("opencode"),
    };
    let project = roots.claude.join("projects/-Users-a-site");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(roots.claude.join("sessions")).unwrap();
    std::fs::write(project.join("s-1.jsonl"), transcript).unwrap();
    let child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    std::fs::write(
        roots.claude.join(format!("sessions/{}.json", child.id())),
        format!(r#"{{"pid":{},"sessionId":"s-1","entrypoint":"{entrypoint}","cwd":"/Users/a/site"}}"#, child.id()),
    )
    .unwrap();
    (roots, child)
}

#[tokio::test]
async fn stops_the_apps_idle_process_before_taking_over() {
    let (roots, mut child) = fake_app_session("idle", IDLE, "claude-desktop");
    let pid = child.id() as i32;
    let decision = take_over(&roots, "s-1", &[]).await.unwrap();
    assert_eq!(decision, Decision::StopAppProcesses(vec![pid]));
    let status = child.wait().unwrap();
    assert!(!status.success(), "the app process was stopped");
}

#[tokio::test]
async fn leaves_a_busy_or_terminal_conversation_alone() {
    let (roots, mut busy) = fake_app_session("busy", MID_TOOL, "claude-desktop");
    assert_eq!(take_over(&roots, "s-1", &[]).await, Err(HandoffError::BusyInApp));
    assert!(busy.try_wait().unwrap().is_none(), "still running");
    busy.kill().unwrap();
    let _ = busy.wait();

    let (roots, mut terminal) = fake_app_session("terminal", IDLE, "cli");
    assert!(matches!(take_over(&roots, "s-1", &[]).await, Err(HandoffError::RunningElsewhere { .. })));
    assert!(terminal.try_wait().unwrap().is_none(), "still running");
    terminal.kill().unwrap();
    let _ = terminal.wait();

    assert_eq!(take_over(&roots, "missing", &[]).await, Err(HandoffError::NoTranscript));
}

/// Prints what the hand-off would decide for every running Claude conversation,
/// without stopping anything.
/// `cargo test --lib handoff_tests::real_decisions -- --ignored --nocapture`
#[test]
#[ignore = "reads the developer's own Claude sessions"]
fn real_decisions() {
    use crate::core::agent_sessions::{claude_processes_for, claude_read_tail, claude_transcript_path};
    let roots = AgentRoots::from_env();
    let registry = std::fs::read_dir(roots.claude.join("sessions")).unwrap();
    for entry in registry.flatten() {
        let raw = std::fs::read_to_string(entry.path()).unwrap_or_default();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else { continue };
        let Some(id) = value["sessionId"].as_str() else { continue };
        let tail = claude_transcript_path(&roots, id)
            .and_then(|path| claude_read_tail(&path, 256 * 1024))
            .unwrap_or_default();
        let activity = turn_activity(&tail);
        let decision = decide(&claude_processes_for(&roots, id), &[], activity);
        println!("{} ({}) {:?} -> {:?}", value["name"], value["cwd"], activity, decision);
    }
}
