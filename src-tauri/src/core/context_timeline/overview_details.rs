/// Newly exposed tracks fetch only their own selected summary/context. A
/// multi-hour app span must not pull every nearby key, OCR row, and screenshot.
async fn overview_slice_detail(
    db: &Arc<Database>, mut slice: ContextSlice,
) -> OverviewResult<ContextSliceDetailDto> {
    let mut payloads = vec![RawPayloadDto {
        label: "Capture summary".to_string(),
        raw_json: pretty_json(serde_json::json!({
            "source": slice.source, "record_count": slice.row_count,
            "start_timestamp": slice.start_timestamp, "end_timestamp": slice.end_timestamp,
            "note": "Overview counts summarize stored records; bar height is not active duration.",
        })),
    }];
    if let Some(table) = match slice.rail.as_str() {
        "keyboard" => Some("keyboard_events"), "mouse" => Some("mouse_events"), _ => None,
    } {
        let sql = format!("SELECT app_name, COUNT(*) FROM {table}
            WHERE timestamp >= ? AND timestamp < ? GROUP BY app_name ORDER BY COUNT(*) DESC LIMIT 20");
        let apps: Vec<(String, i64)> = sqlx::query_as(&sql)
            .bind(slice.start_timestamp).bind(slice.end_timestamp).fetch_all(db.pool()).await?;
        payloads.push(RawPayloadDto {
            label: "Activity by app (up to 20)".to_string(),
            raw_json: pretty_json(serde_json::to_value(apps)?),
        });
    }
    let mut bundle = None;
    if slice.rail == "focus" || slice.rail == "visible_windows" {
        let snapshot_id = slice.id.strip_prefix("visible-").or_else(|| slice.id.strip_prefix("focus-"));
        let columns = "SELECT id, session_id, timestamp, last_seen_at, frontmost_app_name,
                    frontmost_bundle_id, visible_windows_json, confidence, source
             FROM window_snapshots";
        let sql = match snapshot_id {
            Some(_) => format!("{columns} WHERE id = ?"),
            None => format!("{columns} WHERE timestamp <= ?
                AND COALESCE(last_seen_at, timestamp) + 5000 > ?
                AND (? IS NULL OR session_id = ?) ORDER BY timestamp DESC LIMIT 1"),
        };
        let mut query = sqlx::query_as::<_, WindowSnapshotRow>(&sql);
        query = match snapshot_id {
            Some(id) => query.bind(id),
            None => query.bind(slice.start_timestamp).bind(slice.start_timestamp)
                .bind(&slice.session_id).bind(&slice.session_id),
        };
        let snapshot = query.fetch_optional(db.pool()).await?;
        if let Some(snapshot) = snapshot {
            slice.visible_windows = parse_visible_windows(&snapshot)?;
            bundle = snapshot.frontmost_bundle_id;
        }
        // Focus transitions can be captured with running-app snapshots off.
        let event: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT payload_json FROM context_events WHERE id = ?"
        ).bind(&slice.id).fetch_optional(db.pool()).await?;
        if let Some((Some(json),)) = event {
            payloads.push(RawPayloadDto { label: "Focus transition".to_string(), raw_json: json });
        }
    }
    if slice.rail == "system" {
        let event: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT payload_json FROM context_events WHERE id = ?"
        ).bind(&slice.id).fetch_optional(db.pool()).await?;
        if let Some((Some(json),)) = event {
            payloads.push(RawPayloadDto { label: "System event".to_string(), raw_json: json });
        }
    }
    Ok(ContextSliceDetailDto {
        rail_label: detail_rail_label(&slice.rail).to_string(),
        occurred_at: slice.start_timestamp,
        duration_ms: slice.end_timestamp.saturating_sub(slice.start_timestamp).max(1),
        storage_bytes: slice.storage_bytes, storage_exact: false,
        row_count: slice.row_count, file_count: 0, linked_file_paths: Vec::new(),
        focused_app: slice.app_name.clone(), focused_bundle_id: bundle,
        visible_windows: slice.visible_windows.clone(), interaction_reasons: slice.reasons.clone(),
        pii_entities: Vec::new(), raw_payloads: payloads, ocr_reconstruction: None,
        nearby_system_events: Vec::new(), slice,
    })
}
