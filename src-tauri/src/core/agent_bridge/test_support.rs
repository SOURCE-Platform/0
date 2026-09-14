//! Shared helpers for agent_bridge tests.

use crate::core::agent_sessions::AgentRoots;
use std::path::PathBuf;

/// A stand-in for `claude`: replies to every message with a real captured turn.
pub fn fake_claude(name: &str) -> PathBuf {
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

/// Empty app folders under a temp directory, so tests never touch real ones.
pub fn temp_roots(name: &str) -> AgentRoots {
    let base = std::env::temp_dir().join(format!("source_bridge_{name}"));
    let _ = std::fs::remove_dir_all(&base);
    let roots = AgentRoots {
        codex: base.join("codex"),
        claude: base.join("claude"),
        factory: base.join("factory"),
        opencode: base.join("opencode"),
    };
    std::fs::create_dir_all(roots.claude.join("projects/-conversation")).unwrap();
    std::fs::create_dir_all(roots.claude.join("sessions")).unwrap();
    roots
}

/// Write a conversation transcript whose folder exists, so a process can start in it.
pub fn write_transcript(roots: &AgentRoots, session_id: &str, last_records: &str) {
    let cwd = roots.claude.parent().unwrap().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let opening = format!(
        r#"{{"type":"user","cwd":"{}","permissionMode":"acceptEdits","message":{{"content":"hi"}}}}"#,
        cwd.display()
    );
    let path = roots.claude.join(format!("projects/-conversation/{session_id}.jsonl"));
    std::fs::write(path, format!("{opening}\n{last_records}\n")).unwrap();
}
