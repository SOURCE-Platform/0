use super::claude_history::{context_from_text, messages_from_text, recent_messages, transcript_path};
use super::claude_records::{parse_line, ClaudeRecord};
use super::claude_registry::{parse_registry_entry, pid_alive, processes_for};
use super::message_types::MessageRole;
use super::paths::AgentRoots;

const FIXTURE: &str = include_str!("fixtures/claude_conversation.jsonl");

fn temp_claude_root(name: &str) -> AgentRoots {
    let base = std::env::temp_dir().join(format!("source_claude_tests_{name}"));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("claude")).unwrap();
    AgentRoots {
        codex: base.join("codex"),
        claude: base.join("claude"),
        factory: base.join("factory"),
        opencode: base.join("opencode"),
    }
}

#[test]
fn builds_a_readable_conversation_from_a_transcript() {
    let messages = messages_from_text(FIXTURE, 50);
    let summary: Vec<(MessageRole, &str)> =
        messages.iter().map(|m| (m.role, m.text.as_str())).collect();
    assert_eq!(
        summary,
        vec![
            (MessageRole::User, "Make the header scroll away on the way down."),
            (MessageRole::Assistant, "I'll look at the header first."),
            (MessageRole::Tool, "Read"),
            (MessageRole::Peer, "From relay-e7: Please stop."),
            (MessageRole::Assistant, "The header now hides on scroll down.\n\nIt comes back when you scroll up."),
            (MessageRole::User, "Thanks, now the footer."),
        ]
    );
    assert_eq!(messages[0].at_ms, 1789344240100);
}

#[test]
fn hides_thinking_tool_output_subagents_meta_and_slash_commands() {
    let text = messages_from_text(FIXTURE, 50)
        .into_iter()
        .map(|m| m.text)
        .collect::<Vec<_>>()
        .join("|");
    for hidden in ["export function Header", "Subagent notes", "system-reminder", "/clear"] {
        assert!(!text.contains(hidden), "{hidden} should not be shown");
    }
}

#[test]
fn keeps_only_the_most_recent_messages() {
    let messages = messages_from_text(FIXTURE, 2);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].text, "Thanks, now the footer.");
}

#[test]
fn ignores_a_line_cut_in_half_by_reading_the_tail() {
    let cut = &FIXTURE[FIXTURE.find("isSidechain").unwrap()..];
    assert!(parse_line(cut.lines().next().unwrap()).is_empty());
    assert_eq!(messages_from_text(cut, 50).last().unwrap().text, "Thanks, now the footer.");
}

#[test]
fn a_peer_message_is_never_mistaken_for_the_user() {
    let line = FIXTURE.lines().find(|l| l.contains("\"kind\":\"peer\"")).unwrap();
    assert!(matches!(
        parse_line(line).as_slice(),
        [ClaudeRecord::PeerMessage { from_name, body, .. }] if from_name == "relay-e7" && body == "Please stop."
    ));
}

#[test]
fn parses_registry_entries_and_rejects_broken_ones() {
    let entry = parse_registry_entry(
        r#"{"pid":4242,"sessionId":"s-1","cwd":"/Users/a/site","name":"site-a1","entrypoint":"claude-desktop"}"#,
    )
    .unwrap();
    assert_eq!((entry.pid, entry.session_id.as_str(), entry.name.as_str()), (4242, "s-1", "site-a1"));
    assert!(parse_registry_entry(r#"{"pid":0,"sessionId":"s-1"}"#).is_none());
    assert!(parse_registry_entry(r#"{"pid":12,"sessionId":""}"#).is_none());
    assert!(parse_registry_entry("not json").is_none());
}

#[test]
fn a_registry_file_left_by_a_dead_process_is_not_live() {
    let roots = temp_claude_root("registry");
    let sessions = roots.claude.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let me = std::process::id() as i32;
    std::fs::write(sessions.join("mine.json"), format!(r#"{{"pid":{me},"sessionId":"s-live"}}"#)).unwrap();
    // pid 999999 is above macOS's pid ceiling, so it can never be running.
    std::fs::write(sessions.join("dead.json"), r#"{"pid":999999,"sessionId":"s-live"}"#).unwrap();

    assert!(pid_alive(me));
    assert!(!pid_alive(999_999));
    let live = processes_for(&roots, "s-live");
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].pid, me);
}

#[test]
fn finds_a_transcript_in_any_project_folder_and_reads_it() {
    let roots = temp_claude_root("history");
    let project = roots.claude.join("projects/-Users-a-site");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("s-1.jsonl"), FIXTURE).unwrap();

    assert_eq!(transcript_path(&roots, "s-1").unwrap(), project.join("s-1.jsonl"));
    assert!(transcript_path(&roots, "../s-1").is_none(), "ids can't escape the projects folder");
    assert_eq!(recent_messages(&roots, "s-1", 1).unwrap()[0].text, "Thanks, now the footer.");
    assert!(recent_messages(&roots, "missing", 5).is_err());
}

#[test]
fn reads_the_folder_and_latest_permission_mode() {
    let text = r#"{"type":"user","cwd":"/Users/a/old","permissionMode":"default"}
{"type":"assistant","cwd":"/Users/a/site"}
{"type":"user","cwd":"/Users/a/site","permissionMode":"acceptEdits"}
{"type":"last-prompt"}"#;
    let context = context_from_text(text).unwrap();
    assert_eq!(context.cwd, "/Users/a/site");
    assert_eq!(context.permission_mode.as_deref(), Some("acceptEdits"));
    assert!(context_from_text(r#"{"type":"last-prompt"}"#).is_none(), "no folder, no context");
}

#[test]
fn watches_only_files_that_describe_sessions() {
    use super::watch::is_relevant;
    use std::path::Path;
    assert!(is_relevant(Path::new("/h/.claude/sessions/4242.json")));
    assert!(is_relevant(Path::new("/h/.claude/projects/-a/s-1.jsonl")));
    assert!(is_relevant(Path::new("/h/.codex/state_5.sqlite-wal")));
    assert!(is_relevant(Path::new("/h/.local/share/opencode/opencode.db-wal")));
    assert!(is_relevant(Path::new("/h/.factory/sessions-index.json")));
    assert!(!is_relevant(Path::new("/h/.codex/logs_2.sqlite-wal")));
    assert!(!is_relevant(Path::new("/h/.claude/projects/-a/memory/notes.md")));
    assert!(!is_relevant(Path::new("/h/.factory/settings.json")));
}

#[tokio::test]
async fn announces_a_burst_of_changes_once() {
    let roots = temp_claude_root("watch");
    let project = roots.claude.join("projects/-a");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(roots.claude.join("sessions")).unwrap();
    let (_watch, mut changes) = super::watch::watch_agent_changes(&roots).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await; // FSEvents stream starts asynchronously
    for n in 0..5 {
        std::fs::write(project.join("s-1.jsonl"), format!("line {n}\n")).unwrap();
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), changes.changed()).await.unwrap().unwrap();
    assert_eq!(*changes.borrow_and_update(), 1, "five quick writes settle into one change");
}

/// Manual timing check against this machine's real, large transcripts:
/// `cargo test --lib claude_tests::real_history_speed -- --ignored --nocapture`
#[test]
#[ignore = "reads the developer's own Claude transcripts"]
fn real_history_speed() {
    let roots = AgentRoots::from_env();
    let biggest = std::fs::read_dir(roots.claude.join("projects"))
        .unwrap()
        .flatten()
        .flat_map(|project| std::fs::read_dir(project.path()).into_iter().flatten().flatten())
        .filter(|entry| entry.path().extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .max_by_key(|entry| entry.metadata().map(|m| m.len()).unwrap_or(0))
        .unwrap();
    let size = biggest.metadata().unwrap().len();
    let id = biggest.path().file_stem().unwrap().to_string_lossy().to_string();
    let started = std::time::Instant::now();
    let messages = recent_messages(&roots, &id, 40).unwrap();
    let elapsed = started.elapsed();
    println!("{} MB transcript: {} messages in {:?}", size / 1_000_000, messages.len(), elapsed);
    assert!(elapsed.as_millis() < 100, "took {elapsed:?}");
}
