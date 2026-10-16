#[derive(FromRow)]
struct ActivityOverviewRow {
    bucket: i64,
    first: i64,
    last: i64,
    count: i64,
}

async fn activity_overview(
    db: &Arc<Database>, start: i64, end: i64,
    table: &str, rail: &str, label: &str,
) -> OverviewResult<TimelineRailDto> {
    // table is supplied only by the fixed source list below, never by the UI.
    let sql = format!(
        "SELECT (timestamp / ?) * ? AS bucket, MIN(timestamp) AS first,
                MAX(timestamp) AS last, COUNT(*) AS count FROM {table}
         WHERE timestamp >= ? AND timestamp <= ? GROUP BY bucket ORDER BY bucket"
    );
    let bucket_ms = overview_bucket_ms(start, end);
    let rows = sqlx::query_as::<_, ActivityOverviewRow>(&sql)
        .bind(bucket_ms).bind(bucket_ms).bind(start).bind(end)
        .fetch_all(db.pool()).await?;
    let slices = rows.into_iter().map(|row| {
        let mut slice = overview_slice(
            format!("{rail}-activity-{}", row.bucket), rail,
            row.first, row.last.saturating_add(1), format!("{} {} events", row.count, label.to_lowercase()),
        );
        slice.source = table.to_string();
        slice.row_count = row.count as u64;
        slice.tags.push("activity_bucket".to_string());
        slice.reasons.push(format!(
            "{} recorded events summarized in a {}-second bucket. Gaps between buckets remain visible.",
            row.count, bucket_ms / 1000,
        ));
        slice
    }).collect();
    Ok(TimelineRailDto::lane(rail, label,
        "Recorded activity summarized into compact time buckets.",
        "Bar height indicates event count, not exact time spent actively working.", slices))
}

async fn build_activity_overviews(
    db: &Arc<Database>, start: i64, end: i64,
) -> OverviewResult<Vec<TimelineRailDto>> {
    let mut rails = Vec::new();
    for (table, rail, label) in [
        ("keyboard_events", "keyboard", "Keyboard Activity"),
        ("mouse_events", "mouse", "Mouse Activity"),
        ("frames", "evidence", "Screen Evidence"),
        ("visual_scene_snapshots", "vision", "Vision / Scene"),
        ("attention_snapshots", "attention", "Gaze / Attention"),
    ] {
        rails.push(activity_overview(db, start, end, table, rail, label).await?);
    }
    Ok(rails)
}
