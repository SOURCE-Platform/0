/// Store one row for each distinct running-app and focus state. Repeated polls
/// extend the row's observed span without copying the app list again.
pub async fn insert_window_snapshot(
    db: &Arc<Database>,
    session_id: Option<&str>,
    timestamp: i64,
    frontmost_app_name: Option<&str>,
    frontmost_bundle_id: Option<&str>,
    visible_windows: &[WindowSnapshotDto],
    source: &str,
    confidence: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut apps = visible_windows.to_vec();
    apps.sort_by(|a, b| {
        (&a.bundle_id, a.process_id, &a.app_name)
            .cmp(&(&b.bundle_id, b.process_id, &b.app_name))
    });
    let payload = serde_json::to_string(&apps)?;
    let mut tx = db.pool().begin().await?;
    let previous = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, i64)>(
        "SELECT id, visible_windows_json, frontmost_app_name, frontmost_bundle_id, \
         COALESCE(last_seen_at, timestamp) \
         FROM window_snapshots WHERE session_id IS ? ORDER BY timestamp DESC, rowid DESC LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some((id, prior_payload, prior_app, prior_bundle, observed_until)) = previous {
        if prior_payload == payload
            && prior_app.as_deref() == frontmost_app_name
            && prior_bundle.as_deref() == frontmost_bundle_id
            && (0..=15_000).contains(&(timestamp - observed_until))
        {
            sqlx::query("UPDATE window_snapshots SET last_seen_at = ? WHERE id = ?")
                .bind(timestamp)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
    }

    sqlx::query(
        "INSERT INTO window_snapshots (id, session_id, timestamp, frontmost_app_name, \
         frontmost_bundle_id, visible_windows_json, last_seen_at, confidence, source, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(session_id)
    .bind(timestamp)
    .bind(frontmost_app_name)
    .bind(frontmost_bundle_id)
    .bind(payload)
    .bind(timestamp)
    .bind(confidence as f64)
    .bind(source)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
