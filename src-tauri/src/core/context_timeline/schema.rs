pub async fn init_schema(
    db: &Arc<Database>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS context_events (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            timestamp INTEGER NOT NULL,
            channel TEXT NOT NULL,
            event_type TEXT NOT NULL,
            source TEXT NOT NULL,
            confidence REAL NOT NULL,
            payload_json TEXT,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(db.pool())
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS window_snapshots (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            timestamp INTEGER NOT NULL,
            frontmost_app_name TEXT,
            frontmost_bundle_id TEXT,
            visible_windows_json TEXT NOT NULL,
            confidence REAL NOT NULL,
            source TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(db.pool())
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_context_events_time ON context_events(timestamp)")
        .execute(db.pool())
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_context_events_channel ON context_events(channel)")
        .execute(db.pool())
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_window_snapshots_time ON window_snapshots(timestamp)",
    )
    .execute(db.pool())
    .await?;

    Ok(())
}

pub async fn insert_context_event(
    db: &Arc<Database>,
    session_id: Option<&str>,
    timestamp: i64,
    channel: &str,
    event_type: &str,
    source: &str,
    confidence: f32,
    payload_json: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sqlx::query(
        r#"
        INSERT INTO context_events (
            id, session_id, timestamp, channel, event_type, source, confidence, payload_json, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(session_id)
    .bind(timestamp)
    .bind(channel)
    .bind(event_type)
    .bind(source)
    .bind(confidence as f64)
    .bind(payload_json)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(db.pool())
    .await?;

    Ok(())
}

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
    sqlx::query(
        r#"
        INSERT INTO window_snapshots (
            id, session_id, timestamp, frontmost_app_name, frontmost_bundle_id,
            visible_windows_json, confidence, source, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(session_id)
    .bind(timestamp)
    .bind(frontmost_app_name)
    .bind(frontmost_bundle_id)
    .bind(serde_json::to_string(visible_windows)?)
    .bind(confidence as f64)
    .bind(source)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(db.pool())
    .await?;

    Ok(())
}
