use super::{
    AppUsageContextRow, InferredAppContext, KeyboardContextRow, WindowSnapshotContextRow,
    APP_CONTEXT_LOOKBACK_MS,
};
use crate::core::database::Database;
use std::sync::Arc;

pub(super) async fn infer_app_context(
    db: &Arc<Database>,
    timestamp: i64,
    session_id: Option<&str>,
) -> Result<InferredAppContext, Box<dyn std::error::Error + Send + Sync>> {
    let snapshot = sqlx::query_as::<_, WindowSnapshotContextRow>(
        r#"
        SELECT id, timestamp, frontmost_app_name, frontmost_bundle_id
        FROM window_snapshots
        WHERE timestamp <= ? AND timestamp >= ?
        ORDER BY timestamp DESC
        LIMIT 1
        "#,
    )
    .bind(timestamp)
    .bind(timestamp - APP_CONTEXT_LOOKBACK_MS)
    .fetch_optional(db.pool())
    .await?;

    if let Some(snapshot) = snapshot {
        return Ok(InferredAppContext {
            app_name: snapshot.frontmost_app_name,
            bundle_id: snapshot.frontmost_bundle_id,
            window_title: None,
            visible_window_snapshot_id: Some(snapshot.id),
        });
    }

    let query = if session_id.is_some() {
        r#"
        SELECT timestamp, app_name, window_title
        FROM keyboard_events
        WHERE timestamp <= ? AND timestamp >= ? AND session_id = ?
        ORDER BY timestamp DESC
        LIMIT 1
        "#
    } else {
        r#"
        SELECT timestamp, app_name, window_title
        FROM keyboard_events
        WHERE timestamp <= ? AND timestamp >= ?
        ORDER BY timestamp DESC
        LIMIT 1
        "#
    };

    let mut keyboard_query = sqlx::query_as::<_, KeyboardContextRow>(query)
        .bind(timestamp)
        .bind(timestamp - APP_CONTEXT_LOOKBACK_MS);
    if let Some(session_id) = session_id {
        keyboard_query = keyboard_query.bind(session_id);
    }
    if let Some(event) = keyboard_query.fetch_optional(db.pool()).await? {
        return Ok(InferredAppContext {
            app_name: Some(event.app_name),
            bundle_id: None,
            window_title: Some(event.window_title),
            visible_window_snapshot_id: None,
        });
    }

    let app_usage = sqlx::query_as::<_, AppUsageContextRow>(
        r#"
        SELECT app_name, bundle_id
        FROM app_usage
        WHERE start_timestamp <= ?
          AND (end_timestamp IS NULL OR end_timestamp >= ?)
        ORDER BY start_timestamp DESC
        LIMIT 1
        "#,
    )
    .bind(timestamp)
    .bind(timestamp)
    .fetch_optional(db.pool())
    .await?;

    Ok(InferredAppContext {
        app_name: app_usage.as_ref().map(|row| row.app_name.clone()),
        bundle_id: app_usage.as_ref().map(|row| row.bundle_id.clone()),
        window_title: None,
        visible_window_snapshot_id: None,
    })
}
