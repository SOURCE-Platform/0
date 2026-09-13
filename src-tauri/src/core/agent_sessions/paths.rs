use std::path::PathBuf;

/// Where each agent app keeps its session records.
///
/// Kept in one struct so tests can point every reader at a temp directory.
#[derive(Debug, Clone)]
pub struct AgentRoots {
    pub codex: PathBuf,
    pub claude: PathBuf,
    pub factory: PathBuf,
    pub opencode: PathBuf,
}

impl AgentRoots {
    /// Real locations on this Mac, honouring the apps' own env overrides.
    pub fn from_env() -> Self {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()));
        Self {
            codex: env_path("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            claude: env_path("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude")),
            factory: env_path("FACTORY_HOME").unwrap_or_else(|| home.join(".factory")),
            opencode: env_path("OPENCODE_DATA")
                .unwrap_or_else(|| home.join(".local/share/opencode")),
        }
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key).ok().filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Codex numbers its state database (`state_5.sqlite`) and bumps the number on
/// migrations, so pick the highest one present.
pub fn newest_numbered_file(dir: &std::path::Path, prefix: &str, suffix: &str) -> Option<PathBuf> {
    let mut best: Option<(u32, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix(prefix) else { continue };
        let Some(number) = rest.strip_suffix(suffix) else { continue };
        let parsed = number.parse::<u32>().unwrap_or(0);
        if best.as_ref().map(|(n, _)| parsed >= *n).unwrap_or(true) {
            best = Some((parsed, path));
        }
    }
    best.map(|(_, path)| path)
}

/// Open one of another app's SQLite files without ever writing to it.
///
/// A database left in write-ahead-log mode normally needs a shared-memory file
/// to be readable, which a strictly read-only opener cannot always get. When
/// that happens, fall back to reading the main file alone: the newest turns may
/// be a moment stale, which is far better than showing the app as empty.
pub async fn read_only_sqlite(
    path: &std::path::Path,
) -> Result<sqlx::SqliteConnection, sqlx::Error> {
    use sqlx::{sqlite::SqliteConnectOptions, ConnectOptions};
    let base = SqliteConnectOptions::new().filename(path).read_only(true);
    match base.clone().connect().await {
        Ok(conn) => Ok(conn),
        Err(error) => {
            eprintln!(
                "[agent_sessions] {} not readable ({error}); retrying without the write-ahead log",
                path.display()
            );
            base.immutable(true).connect().await
        }
    }
}
