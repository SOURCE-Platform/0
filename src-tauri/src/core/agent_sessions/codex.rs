use super::paths::{newest_numbered_file, read_only_sqlite, AgentRoots};
use super::types::{project_name_from_path, shorten, title_or_prompt, AgentApp, AgentSession};
use sqlx::Row;

/// Codex keeps every thread in `~/.codex/state_<n>.sqlite`, which the ChatGPT
/// app and the CLI share. Reading it needs no running Codex process.
pub async fn list(roots: &AgentRoots, limit: usize) -> Result<Vec<AgentSession>, String> {
    let Some(db) = newest_numbered_file(&roots.codex, "state_", ".sqlite") else {
        return Ok(Vec::new());
    };
    let mut conn = read_only_sqlite(&db).await.map_err(|e| e.to_string())?;

    // `recency_at_ms` is the column the app itself sorts by; the coalesce chain
    // keeps this working against older schemas that only had seconds.
    //
    // Threads with no title, preview or first message are empty shells the app
    // opened and never used, so they are left out rather than filtered on
    // `has_user_event`, which this Codex version no longer maintains.
    let rows = sqlx::query(
        "SELECT id, \
                COALESCE(NULLIF(name, ''), NULLIF(title, ''), '') AS title, \
                COALESCE(NULLIF(preview, ''), NULLIF(first_user_message, ''), '') AS preview, \
                cwd, \
                COALESCE(NULLIF(recency_at_ms, 0), NULLIF(updated_at_ms, 0), updated_at * 1000) AS updated_ms, \
                archived \
         FROM threads \
         WHERE title <> '' OR preview <> '' \
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
            let preview: String = row.try_get("preview").unwrap_or_default();
            let project_path: String = row.try_get("cwd").unwrap_or_default();
            AgentSession {
                title: title_or_prompt(&title, &preview, &id),
                id,
                app: AgentApp::Codex,
                project_name: project_name_from_path(&project_path),
                project_path,
                updated_at_ms: row.try_get::<i64, _>("updated_ms").unwrap_or(0),
                preview: shorten(&preview, 140),
                live: false,
                archived: row.try_get::<i64, _>("archived").unwrap_or(0) != 0,
            }
        })
        .collect())
}
