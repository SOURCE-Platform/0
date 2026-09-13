use super::paths::{read_only_sqlite, AgentRoots};
use super::types::{project_name_from_path, shorten, title_or_prompt, AgentApp, AgentSession};
use sqlx::Row;

/// OpenCode keeps sessions in one SQLite file shared by its desktop app, its
/// TUI and any `opencode serve` process.
pub async fn list(roots: &AgentRoots, limit: usize) -> Result<Vec<AgentSession>, String> {
    let db = roots.opencode.join("opencode.db");
    if !db.exists() {
        return Ok(Vec::new());
    }
    let mut conn = read_only_sqlite(&db).await.map_err(|e| e.to_string())?;

    let rows = sqlx::query(
        "SELECT id, COALESCE(title, '') AS title, COALESCE(directory, '') AS directory, \
                COALESCE(time_updated, time_created, 0) AS updated_ms \
         FROM session \
         ORDER BY updated_ms DESC \
         LIMIT ?",
    )
    .bind(limit as i64)
    .fetch_all(&mut conn)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let id: String = row.try_get("id").unwrap_or_default();
            let title: String = row.try_get("title").unwrap_or_default();
            let project_path: String = row.try_get("directory").unwrap_or_default();
            AgentSession {
                title: title_or_prompt(&title, "", &id),
                id,
                app: AgentApp::OpenCode,
                project_name: project_name_from_path(&project_path),
                project_path,
                updated_at_ms: row.try_get::<i64, _>("updated_ms").unwrap_or(0),
                preview: shorten(&title, 140),
                live: false,
                archived: false,
            }
        })
        .collect())
}
