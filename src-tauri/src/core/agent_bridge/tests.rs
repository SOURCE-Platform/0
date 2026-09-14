use super::claude_cli::{newest_bundled, resume_args};
use super::stream_protocol::{encode_user_message, parse_line, StreamEvent};

const STREAM: &str = include_str!("fixtures/claude_stream.jsonl");
const AUTH_ERROR: &str = include_str!("fixtures/claude_stream_auth_error.jsonl");

fn events(text: &str) -> Vec<StreamEvent> {
    text.lines().flat_map(parse_line).collect()
}

#[test]
fn reads_a_real_turn_in_order() {
    let kinds: Vec<String> = events(STREAM)
        .iter()
        .map(|event| match event {
            StreamEvent::Init { .. } => "init".to_string(),
            StreamEvent::AssistantText { text } => format!("text:{text}"),
            StreamEvent::ToolUse { name } => format!("tool:{name}"),
            StreamEvent::ToolResult { is_error } => format!("result_ok:{}", !is_error),
            StreamEvent::Progress { detail } => format!("progress:{detail}"),
            StreamEvent::TurnSummary { detail, .. } => format!("summary:{detail}"),
            StreamEvent::Usage { .. } => "usage".to_string(),
            StreamEvent::TurnDone(result) => format!("done:{}", result.text),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "init",
            "tool:Bash",
            "usage",
            "progress:Running echo hi",
            "result_ok:true",
            "text:DONE",
            "usage",
            "summary:ran echo hi; replied DONE",
            "done:DONE",
        ]
    );
}

#[test]
fn reads_session_usage_and_turn_details() {
    let all = events(STREAM);
    assert!(all.contains(&StreamEvent::Init {
        session_id: "bc54654d-32bd-4310-acdd-4c2107bd2b68".to_string(),
        model: "claude-haiku-4-5-20251001".to_string(),
    }));
    let usage = all.iter().find_map(|e| match e {
        StreamEvent::Usage { status, weekly_utilization } => Some((status.clone(), *weekly_utilization)),
        _ => None,
    });
    assert_eq!(usage, Some(("allowed_warning".to_string(), Some(0.79))));
    let done = all.iter().find_map(|e| match e {
        StreamEvent::TurnDone(result) => Some(result.clone()),
        _ => None,
    });
    let done = done.unwrap();
    assert!(!done.is_error && !done.is_auth_error());
    assert_eq!((done.duration_ms, done.queued_turn_count), (4315, 0));
}

#[test]
fn recognises_an_expired_sign_in() {
    match events(AUTH_ERROR).as_slice() {
        [StreamEvent::TurnDone(result)] => assert!(result.is_auth_error()),
        other => panic!("expected one turn result, got {other:?}"),
    }
}

#[test]
fn ignores_junk_and_subagent_lines() {
    assert!(parse_line("not json").is_empty());
    assert!(parse_line(
        r#"{"type":"assistant","parent_tool_use_id":"toolu_1","message":{"content":[{"type":"text","text":"sub"}]}}"#
    )
    .is_empty());
}

#[test]
fn encodes_text_safely() {
    let line = encode_user_message("Say \"hi\"\nthen a backslash \\ done");
    assert!(!line.contains('\n'), "one message must stay on one line");
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["message"]["content"], "Say \"hi\"\nthen a backslash \\ done");
    assert_eq!(value["type"], "user");
}

#[test]
fn picks_the_newest_bundled_claude_by_version_number() {
    let root = std::env::temp_dir().join("source_agent_bridge_cli");
    let _ = std::fs::remove_dir_all(&root);
    for version in ["2.1.99", "2.1.266", "2.1.260", "not-a-version"] {
        let dir = root.join(version).join("claude.app/Contents/MacOS");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("claude"), b"").unwrap();
    }
    // A newer version folder without the program inside doesn't count.
    std::fs::create_dir_all(root.join("2.2.0")).unwrap();
    let found = newest_bundled(&root).unwrap();
    assert!(found.starts_with(root.join("2.1.266")), "{found:?}");
}

#[test]
fn resume_args_inherit_a_safe_permission_mode_only() {
    let base = resume_args("s-1", None);
    assert_eq!(&base[..3], ["-p", "--resume", "s-1"]);
    assert!(!base.contains(&"--permission-mode".to_string()));
    assert!(resume_args("s-1", Some("acceptEdits")).ends_with(&["--permission-mode".to_string(), "acceptEdits".to_string()]));
    assert!(!resume_args("s-1", Some("bypassPermissions")).contains(&"bypassPermissions".to_string()));
}
