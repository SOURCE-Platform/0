use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use crate::core::database::Database;

use super::storage::StorageResult;
use super::storage_helpers::session_path;

pub(super) async fn delete_session(
    base_path: &Path,
    db: Arc<Database>,
    session_id: Uuid,
) -> StorageResult<()> {
    let session_id_string = session_id.to_string();

    sqlx::query("DELETE FROM ocr_context_entities WHERE session_id = ?")
        .bind(&session_id_string)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_text_spans WHERE session_id = ?")
        .bind(&session_id_string)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_scene_snapshots WHERE session_id = ?")
        .bind(&session_id_string)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM ocr_results WHERE session_id = ?")
        .bind(&session_id_string)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM frames WHERE session_id = ?")
        .bind(&session_id_string)
        .execute(db.pool())
        .await?;
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(&session_id_string)
        .execute(db.pool())
        .await?;

    let path = session_path(base_path, &session_id);
    if path.exists() {
        std::fs::remove_dir_all(&path)?;
    }

    println!("Deleted recording session: {}", session_id);
    Ok(())
}
