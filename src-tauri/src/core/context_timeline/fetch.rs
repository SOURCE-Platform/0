async fn infer_app_context(
    db: &Arc<Database>,
    timestamp: i64,
    session_id: Option<&str>,
) -> Result<InferredAppContext, Box<dyn std::error::Error + Send + Sync>> {
    let snapshot = sqlx::query_as::<_, WindowSnapshotRow>(
        r#"
        SELECT id, session_id, timestamp, frontmost_app_name, frontmost_bundle_id, visible_windows_json, confidence, source
        FROM window_snapshots
        WHERE timestamp <= ?
        ORDER BY timestamp DESC
        LIMIT 1
        "#,
    )
    .bind(timestamp)
    .fetch_optional(db.pool())
    .await?;

    if let Some(snapshot) = snapshot {
        return Ok(InferredAppContext {
            app_name: snapshot.frontmost_app_name,
            bundle_id: snapshot.frontmost_bundle_id,
            window_title: None,
        });
    }

    let session_clause = if session_id.is_some() {
        "AND session_id = ?"
    } else {
        ""
    };
    let keyboard_query = format!(
        r#"
        SELECT timestamp, app_name, window_title
        FROM keyboard_events
        WHERE timestamp <= ? {}
        ORDER BY timestamp DESC
        LIMIT 1
        "#,
        session_clause
    );
    let mut keyboard_query_builder =
        sqlx::query_as::<_, KeyboardEventSummaryRow>(&keyboard_query).bind(timestamp);
    if let Some(session_id) = session_id {
        keyboard_query_builder = keyboard_query_builder.bind(session_id);
    }
    if let Some(event) = keyboard_query_builder.fetch_optional(db.pool()).await? {
        return Ok(InferredAppContext {
            app_name: Some(event.app_name),
            bundle_id: None,
            window_title: Some(event.window_title),
        });
    }

    let app_usage = sqlx::query_as::<_, AppUsageRow>(
        r#"
        SELECT session_id, app_name, bundle_id, process_id, start_timestamp, end_timestamp
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

    if let Some(app) = app_usage {
        return Ok(InferredAppContext {
            app_name: Some(app.app_name),
            bundle_id: Some(app.bundle_id),
            window_title: None,
        });
    }

    Ok(InferredAppContext {
        app_name: None,
        bundle_id: None,
        window_title: None,
    })
}

async fn get_sessions(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<SessionRow>, Box<dyn std::error::Error + Send + Sync>> {
    let sessions = sqlx::query_as::<_, SessionRow>(
        r#"
        SELECT id, start_timestamp, end_timestamp
        FROM sessions
        WHERE start_timestamp <= ?
          AND (end_timestamp IS NULL OR end_timestamp >= ?)
        ORDER BY start_timestamp ASC
        "#,
    )
    .bind(end_timestamp)
    .bind(start_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(sessions)
}

async fn get_context_events(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<ContextEventRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, ContextEventRow>(
        r#"
        SELECT id, session_id, timestamp, channel, event_type, source, confidence, payload_json
        FROM context_events
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_window_snapshots(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<WindowSnapshotRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, WindowSnapshotRow>(
        r#"
        SELECT id, session_id, timestamp, frontmost_app_name, frontmost_bundle_id, visible_windows_json, confidence, source
        FROM window_snapshots
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_keyboard_events(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<KeyboardEventSummaryRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, KeyboardEventSummaryRow>(
        r#"
        SELECT timestamp, app_name, window_title
        FROM keyboard_events
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_mouse_events(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<MouseEventSummaryRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, MouseEventSummaryRow>(
        r#"
        SELECT timestamp, app_name, window_title
        FROM mouse_events
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_ocr_rows(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<OcrRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, OcrRow>(
        r#"
        SELECT id, session_id, timestamp, frame_path, text, confidence, bounding_box
        FROM ocr_results
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        LIMIT 500
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    Ok(rows)
}

async fn get_frame_rows(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<FrameRow>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = sqlx::query_as::<_, FrameRow>(
        r#"
        SELECT session_id, timestamp, file_path
        FROM frames
        WHERE timestamp >= ? AND timestamp <= ?
        ORDER BY timestamp ASC
        LIMIT 120
        "#,
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await
    .unwrap_or_default();
    Ok(rows)
}

pub fn app_infos_to_visible_windows(
    frontmost: Option<&AppInfo>,
    running_apps: &[AppInfo],
) -> Vec<WindowSnapshotDto> {
    running_apps
        .iter()
        .map(|app| WindowSnapshotDto {
            app_name: app.name.clone(),
            bundle_id: app.bundle_id.clone(),
            process_id: app.process_id,
            is_frontmost: frontmost
                .map(|item| item.process_id == app.process_id)
                .unwrap_or(false),
            confidence: if frontmost
                .map(|item| item.process_id == app.process_id)
                .unwrap_or(false)
            {
                0.95
            } else {
                0.45
            },
        })
        .collect()
}
