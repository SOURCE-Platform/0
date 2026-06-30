pub async fn get_context_inspector(
    db: &Arc<Database>,
    timestamp: i64,
) -> Result<ContextInspectorDto, Box<dyn std::error::Error + Send + Sync>> {
    let snapshots = get_window_snapshots(db, timestamp - 60_000, timestamp + 60_000).await?;
    let ocr_rows = get_ocr_rows(db, timestamp - 60_000, timestamp + 60_000).await?;
    let keyboard = get_keyboard_events(db, timestamp - 30_000, timestamp + 30_000).await?;
    let mouse = get_mouse_events(db, timestamp - 30_000, timestamp + 30_000).await?;
    let frames = get_frame_rows(db, timestamp - 60_000, timestamp + 60_000).await?;
    let events = get_context_events(db, timestamp - 120_000, timestamp + 120_000).await?;

    let snapshot = snapshots
        .iter()
        .min_by_key(|row| (row.timestamp - timestamp).abs());

    let focused_app = snapshot.and_then(|row| row.frontmost_app_name.clone());
    let focused_bundle_id = snapshot.and_then(|row| row.frontmost_bundle_id.clone());
    let visible_windows = snapshot
        .map(parse_visible_windows)
        .transpose()?
        .unwrap_or_default();

    let interaction_slice = build_interaction_rail(
        &keyboard,
        &mouse,
        &ocr_rows,
        &snapshots,
        timestamp - DEFAULT_BUCKET_MS,
        timestamp + DEFAULT_BUCKET_MS,
    )
    .slices
    .into_iter()
    .next();

    let nearby_ocr: Vec<OcrRow> = ocr_rows
        .into_iter()
        .filter(|row| (row.timestamp - timestamp).abs() <= 30_000)
        .collect();
    let mut pii_entities = Vec::new();
    for row in &nearby_ocr {
        pii_entities.extend(detect_pii_entities(
            row,
            &infer_app_context(db, row.timestamp, Some(&row.session_id)).await?,
        ));
    }

    let nearby_system_events = events
        .into_iter()
        .filter(|row| row.channel == "system" && (row.timestamp - timestamp).abs() <= 120_000)
        .map(|row| event_row_to_slice(&row))
        .collect();

    Ok(ContextInspectorDto {
        timestamp,
        focused_app,
        focused_bundle_id,
        visible_windows,
        interaction_state: interaction_slice
            .as_ref()
            .and_then(|slice| slice.interaction_state.clone()),
        interaction_reasons: interaction_slice
            .as_ref()
            .map(|slice| slice.reasons.clone())
            .unwrap_or_default(),
        recent_input_state: if keyboard.is_empty() && mouse.is_empty() {
            "No direct keyboard or mouse input was observed in the current inspection window."
                .to_string()
        } else {
            format!(
                "{} keyboard events and {} mouse events nearby",
                keyboard.len(),
                mouse.len()
            )
        },
        ocr_text: nearby_ocr.into_iter().map(|row| row.text).collect(),
        pii_entities,
        evidence_frame_path: frames
            .into_iter()
            .min_by_key(|row| (row.timestamp - timestamp).abs())
            .map(|row| row.file_path),
        nearby_system_events,
    })
}
