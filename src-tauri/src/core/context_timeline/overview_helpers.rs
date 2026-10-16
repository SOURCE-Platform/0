type OverviewResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn overview_slice(id: String, rail: &str, start: i64, end: i64, title: String) -> ContextSlice {
    ContextSlice {
        id, rail: rail.to_string(), slice_kind: "span".to_string(),
        start_timestamp: start, end_timestamp: end.max(start.saturating_add(1)), title,
        subtitle: None, source: "capture_overview".to_string(), confidence: 1.0,
        session_id: None, app_name: None, window_title: None, interaction_state: None,
        reasons: Vec::new(), visible_windows: Vec::new(), ocr_preview: None, pii_count: 0,
        evidence_frame_path: None, storage_bytes: 0, storage_exact: false,
        row_count: 1, file_count: 0, has_detail_view: true, tags: vec!["overview".to_string()],
    }
}

// Clock-aligned buckets remain stable while panning. A day has at most 600
// buckets per high-frequency source; individual records stay in the database.
fn overview_bucket_ms(start: i64, end: i64) -> i64 {
    let needed = end.saturating_sub(start).max(1).saturating_add(599) / 600;
    (needed.saturating_add(29_999) / 30_000).max(1) * 30_000
}
