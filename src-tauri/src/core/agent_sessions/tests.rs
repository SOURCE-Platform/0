use super::claude::parse_tail;
use super::paths::{newest_numbered_file, AgentRoots};
use super::types::{project_name_from_path, shorten, title_or_prompt};
use super::*;

/// A scratch copy of the four app directories, so tests never read real ones.
fn temp_roots(name: &str) -> AgentRoots {
    let base = std::env::temp_dir().join(format!("source_agent_sessions_{name}"));
    let _ = std::fs::remove_dir_all(&base);
    let roots = AgentRoots {
        codex: base.join("codex"),
        claude: base.join("claude"),
        factory: base.join("factory"),
        opencode: base.join("opencode"),
    };
    for dir in [&roots.codex, &roots.claude, &roots.factory, &roots.opencode] {
        std::fs::create_dir_all(dir).expect("create fixture dir");
    }
    roots
}

#[test]
fn shortens_long_text_and_collapses_whitespace() {
    assert_eq!(shorten("  make   the  header\nsticky ", 40), "make the header sticky");
    let long = shorten(&"a".repeat(50), 10);
    assert_eq!(long.chars().count(), 10);
    assert!(long.ends_with('…'));
}

#[test]
fn falls_back_from_title_to_prompt_to_id() {
    assert_eq!(title_or_prompt("Marketing site", "ignored", "abcdefgh"), "Marketing site");
    assert_eq!(title_or_prompt("  ", "fix the nav", "abcdefgh"), "fix the nav");
    assert_eq!(title_or_prompt("", "", "abcdefgh12345"), "Untitled session abcdefgh");
}

#[test]
fn project_name_is_the_folder() {
    assert_eq!(project_name_from_path("/Users/a/Documents/source mobile"), "source mobile");
    assert_eq!(project_name_from_path(""), "");
}

#[test]
fn picks_the_highest_numbered_state_file() {
    let roots = temp_roots("numbered");
    for name in ["state_2.sqlite", "state_11.sqlite", "state_7.sqlite", "notes.txt"] {
        std::fs::write(roots.codex.join(name), b"x").unwrap();
    }
    let found = newest_numbered_file(&roots.codex, "state_", ".sqlite").unwrap();
    assert_eq!(found.file_name().unwrap(), "state_11.sqlite");
}

#[test]
fn reads_factory_index_and_skips_empty_sessions() {
    let roots = temp_roots("factory");
    std::fs::write(
        roots.factory.join("sessions-index.json"),
        r#"{"version":2,"entries":[
            {"sessionId":"old","title":"older work","cwd":"/Users/a/one","mtime":1000.5,"messagesCount":4},
            {"sessionId":"new","title":"newer work","cwd":"/Users/a/two","mtime":2000.5,"messagesCount":2,
             "archivedAt":"2026-08-03T16:01:36.886Z"},
            {"sessionId":"empty","title":"never used","cwd":"/Users/a/three","mtime":3000.0,"messagesCount":0}
        ]}"#,
    )
    .unwrap();

    let sessions = super::factory::list(&roots, 10).unwrap();
    assert_eq!(sessions.len(), 2, "sessions with no messages are skipped");
    assert_eq!(sessions[0].id, "new", "newest first");
    assert_eq!(sessions[0].updated_at_ms, 2000);
    assert!(sessions[0].archived);
    assert_eq!(sessions[1].project_name, "one");
    assert!(!sessions[1].archived);
}

#[test]
fn reads_claude_transcript_tail() {
    let text = r#"{"type":"user","cwd":"/Users/a/site","message":{"role":"user","content":"make the header sticky"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done, the header sticks now."}]}}
{"type":"custom-title","customTitle":"Marketing site header"}"#;
    let parsed = parse_tail(text);
    assert_eq!(parsed.title, "Marketing site header");
    assert_eq!(parsed.preview, "Done, the header sticks now.");
    assert_eq!(parsed.cwd, "/Users/a/site");
    assert_eq!(parsed.prompt, "make the header sticky");
}

#[test]
fn claude_list_marks_running_sessions_live() {
    let roots = temp_roots("claude");
    let project = roots.claude.join("projects/-Users-a-site");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(roots.claude.join("sessions")).unwrap();
    std::fs::write(
        project.join("aaaa1111-2222-3333-4444-555566667777.jsonl"),
        "{\"type\":\"user\",\"cwd\":\"/Users/a/site\",\"message\":{\"role\":\"user\",\"content\":\"fix the nav\"}}\n",
    )
    .unwrap();
    std::fs::write(
        roots.claude.join("sessions/4242.json"),
        r#"{"pid":4242,"sessionId":"aaaa1111-2222-3333-4444-555566667777","name":"site-a1","kind":"interactive"}"#,
    )
    .unwrap();

    let sessions = super::claude::list(&roots, 10).unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].live, "a session with a registry entry is running");
    assert_eq!(sessions[0].title, "site-a1", "the running session's own name is used");
    assert_eq!(sessions[0].project_name, "site");
}

/// Manual check against this machine's real session files:
/// `cargo test --lib agent_sessions::tests::real_machine -- --ignored --nocapture`
#[tokio::test]
#[ignore = "reads the developer's own agent apps"]
async fn real_machine() {
    let sessions = list_agent_sessions(8).await;
    for session in &sessions {
        println!(
            "{:<11} {:<24} {:<18} live={} {}",
            session.app.label(),
            super::types::shorten(&session.title, 24),
            super::types::shorten(&session.project_name, 18),
            session.live,
            chrono::DateTime::from_timestamp_millis(session.updated_at_ms)
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "?".into()),
        );
    }
    println!("{} sessions", sessions.len());
}

#[tokio::test]
async fn missing_apps_produce_an_empty_list_not_an_error() {
    let roots = temp_roots("missing");
    assert!(list_from(&roots, 10).await.is_empty());
}
