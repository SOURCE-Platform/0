#[derive(FromRow)]
struct DesktopOverviewRow {
    id: String,
    session_id: Option<String>,
    timestamp: i64,
    last_seen_at: Option<i64>,
    frontmost_app_name: Option<String>,
    frontmost_bundle_id: Option<String>,
    app_count: i64,
    confidence: f64,
    source: String,
}

async fn build_desktop_overviews(
    db: &Arc<Database>, start: i64, end: i64,
) -> OverviewResult<Vec<TimelineRailDto>> {
    let sessions = get_sessions(db, start, end).await?;
    let session_ends = session_end_lookup(&sessions);
    let events = get_context_events(db, start, end).await?;
    let system = build_system_rail(&sessions, &events, end);
    // SQLite counts apps without transferring/parsing the window-list JSON in
    // the webview or loading OCR, keyboard rows, or screenshots alongside it.
    let rows = sqlx::query_as::<_, DesktopOverviewRow>(
        "WITH spans AS (
            SELECT id, session_id, timestamp, last_seen_at, frontmost_app_name,
                   frontmost_bundle_id, visible_windows_json, confidence, source
            FROM window_snapshots WHERE timestamp BETWEEN ? AND ?
            UNION ALL
            SELECT id, session_id, timestamp, last_seen_at, frontmost_app_name,
                   frontmost_bundle_id, visible_windows_json, confidence, source
            FROM window_snapshots INDEXED BY idx_window_snapshots_last_seen
            WHERE last_seen_at >= ? AND timestamp < ?
         )
         SELECT id, session_id, timestamp, last_seen_at, frontmost_app_name,
                frontmost_bundle_id, json_array_length(visible_windows_json) AS app_count,
                confidence, source FROM spans
         ORDER BY timestamp"
    ).bind(start.saturating_sub(5_000)).bind(end)
        .bind(start.saturating_sub(5_000)).bind(start.saturating_sub(5_000))
        .fetch_all(db.pool()).await?;
    let mut running = Vec::new();
    let mut focus = Vec::new();
    for row in rows {
        let observed_end = row.last_seen_at.unwrap_or(row.timestamp).saturating_add(5_000)
            .min(row.session_id.as_ref().and_then(|id| session_ends.get(id)).copied().unwrap_or(end))
            .min(end);
        if observed_end <= start { continue; }
        let mut slice = overview_slice(format!("visible-{}", row.id), "visible_windows",
            row.timestamp, observed_end, format!("{} running apps", row.app_count));
        slice.app_name = row.frontmost_app_name.clone();
        slice.session_id = row.session_id.clone();
        slice.source = row.source.clone();
        slice.confidence = row.confidence as f32;
        slice.subtitle = row.frontmost_app_name.clone().map(|name| format!("Frontmost: {name}"));
        running.push(slice);
        // Metadata-only snapshots let the existing span builder merge unchanged
        // focus without reading every saved running-app list.
        focus.push(WindowSnapshotRow {
            id: row.id, session_id: row.session_id, timestamp: row.timestamp,
            last_seen_at: row.last_seen_at, frontmost_app_name: row.frontmost_app_name,
            frontmost_bundle_id: row.frontmost_bundle_id, visible_windows_json: "[]".to_string(),
            confidence: row.confidence, source: row.source,
        });
    }
    let mut focus_rail = build_focus_rail(&focus, &sessions, end);
    // Focus recording works independently of running-app snapshots. Include the
    // preceding transition so a span crossing the left edge remains visible.
    let changes = sqlx::query_as::<_, ContextEventRow>(
        "SELECT id, session_id, timestamp, channel, event_type, source, confidence, payload_json
         FROM context_events WHERE channel = 'focus' AND timestamp <= ?
           AND (timestamp >= ? OR id = (SELECT id FROM context_events
                WHERE channel = 'focus' AND timestamp < ? ORDER BY timestamp DESC LIMIT 1))
         ORDER BY timestamp"
    ).bind(end).bind(start).bind(start).fetch_all(db.pool()).await?;
    if !changes.is_empty() {
        focus_rail.slices = changes.iter().enumerate().filter_map(|(index, event)| {
            let session_end = event.session_id.as_ref().and_then(|id| session_ends.get(id)).copied();
            // An old transition whose session is no longer in this range must
            // not fabricate focus across a gap in capture.
            if event.timestamp < start && !sessions.iter().any(|s| Some(&s.id) == event.session_id.as_ref()) {
                return None;
            }
            let stop = changes.get(index + 1).map(|next| next.timestamp).unwrap_or(end)
                .min(session_end.unwrap_or(end)).min(end);
            if stop <= start || stop <= event.timestamp { return None; }
            let mut slice = event_row_to_slice(event);
            slice.end_timestamp = stop;
            slice.slice_kind = "span".to_string();
            slice.tags.push("overview".to_string());
            Some(slice)
        }).collect();
    }
    for slice in &mut focus_rail.slices {
        slice.storage_bytes = 0;
        slice.storage_exact = false;
        if !slice.tags.iter().any(|tag| tag == "overview") { slice.tags.push("overview".to_string()); }
    }
    let mut system = system;
    for slice in &mut system.slices { slice.tags.push("overview".to_string()); }
    Ok(vec![focus_rail, TimelineRailDto::lane("visible_windows", "Running Apps",
        "Observed running-app sets, shown as spans until the set changes.",
        "Running apps are a best-effort proxy for visible windows, not a complete window history.", running), system])
}
