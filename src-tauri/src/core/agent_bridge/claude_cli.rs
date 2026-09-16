use std::path::{Path, PathBuf};

/// The `claude` program SOURCE runs to continue a conversation.
///
/// A packaged Mac app starts with a minimal PATH, so `claude` can't be found by
/// name. Order: an explicit override, then the newest copy the Claude desktop app
/// keeps up to date, then the usual install locations.
pub fn resolve_claude_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("SOURCE_CLAUDE_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let home = PathBuf::from(std::env::var("HOME").ok()?);
    let bundled_root = home.join("Library/Application Support/Claude/claude-code");
    if let Some(path) = newest_bundled(&bundled_root) {
        return Some(path);
    }
    [
        home.join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

/// Inherited settings to remove before starting `claude`.
///
/// A Claude Code host (the Claude desktop app, or a Claude session SOURCE was
/// launched from) gives the `claude` it runs its session id, a private channel
/// back to the host, and "the host renews your sign-in". A `claude` SOURCE
/// starts with those acts as part of that host's session: it waits for a
/// sign-in renewal nobody answers, fails, and the Claude app then asks you to
/// sign in again. SOURCE's `claude` must run on its own saved sign-in.
pub fn inherited_host_settings(names: impl IntoIterator<Item = String>) -> Vec<String> {
    names.into_iter().filter(|name| is_host_setting(name)).collect()
}

fn is_host_setting(name: &str) -> bool {
    name.starts_with("CLAUDE_CODE_")
        || name.starts_with("CLAUDE_AGENT_SDK")
        || matches!(
            name,
            "CLAUDECODE"
                | "CLAUDE_PID"
                | "CLAUDE_EFFORT"
                | "CLAUDE_PREVIEW_CLASSIFIER_FLOOR"
                | "CLAUDE_BRIDGE_OAUTH_TOKEN"
                | "USE_LOCAL_OAUTH"
                | "USE_STAGING_OAUTH"
                | "ANTHROPIC_BASE_URL"
                | "API_TIMEOUT_MS"
        )
}

/// `<root>/<version>/claude.app/Contents/MacOS/claude` with the highest version.
pub(crate) fn newest_bundled(root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let version = parse_version(&entry.file_name().to_string_lossy())?;
            let binary = entry.path().join("claude.app/Contents/MacOS/claude");
            binary.is_file().then_some((version, binary))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, binary)| binary)
}

/// "2.1.266" -> [2, 1, 266]. Compared numerically, so 2.1.266 beats 2.1.99.
fn parse_version(name: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = name.split('.').map(|part| part.parse().ok()).collect();
    parts.filter(|parts| !parts.is_empty())
}

/// Arguments that continue an existing conversation over stream-json.
///
/// `permission_mode` should be the mode the conversation last ran in (read from
/// its transcript), so a voice prompt is allowed exactly what a typed one would be.
pub fn resume_args(session_id: &str, permission_mode: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--resume",
        session_id,
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    if let Some(mode) = permission_mode.filter(|mode| ALLOWED_MODES.contains(mode)) {
        args.push("--permission-mode".to_string());
        args.push(mode.to_string());
    }
    args
}

/// Modes a voice prompt may inherit. `bypassPermissions` is deliberately absent:
/// SOURCE never grants more than asking would.
const ALLOWED_MODES: [&str; 4] = ["default", "acceptEdits", "plan", "auto"];
