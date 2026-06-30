fn build_attention_rail(
    attention_snapshots: &[gaze::AttentionSnapshotDto],
    attention_spans: &[gaze::AttentionSpanDto],
) -> TimelineRailDto {
    let mut slices = attention_spans
        .iter()
        .map(|span| ContextSlice {
            id: span.attention_span_id.clone(),
            rail: "attention".to_string(),
            slice_kind: "span".to_string(),
            start_timestamp: span.first_seen_at,
            end_timestamp: span.last_seen_at.max(span.first_seen_at + 1),
            title: preview_text(&span.label),
            subtitle: Some(format_duration_short(span.duration_ms)),
            source: span.source_id.clone(),
            confidence: span.avg_confidence,
            session_id: Some(span.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![
                "Derived from webcam-based gaze samples mapped onto OCR and context surfaces."
                    .to_string(),
            ],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: estimate_attention_span_storage_bytes(span),
            storage_exact: false,
            row_count: span.supporting_attention_snapshot_ids.len() as u64,
            file_count: 0,
            has_detail_view: true,
            tags: vec![
                "attention".to_string(),
                span.target_type.clone(),
                span.label.clone(),
            ],
        })
        .collect::<Vec<_>>();

    for snapshot in attention_snapshots {
        let headline = snapshot
            .likely_targets
            .first()
            .map(|target| preview_text(&target.label))
            .unwrap_or_else(|| "Unresolved attention sample".to_string());
        slices.push(ContextSlice {
            id: snapshot.attention_snapshot_id.clone(),
            rail: "attention".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: snapshot.timestamp,
            end_timestamp: snapshot.timestamp + 2_000,
            title: headline,
            subtitle: Some(format!(
                "gaze {:.0}% · {} targets",
                snapshot.confidence * 100.0,
                snapshot.likely_targets.len()
            )),
            source: snapshot.source_id.clone(),
            confidence: snapshot.confidence,
            session_id: Some(snapshot.session_id.clone()),
            app_name: snapshot.frontmost_app_name.clone(),
            window_title: snapshot.window_title.clone(),
            interaction_state: None,
            reasons: vec!["Resolved from the latest active gaze calibration plus nearby OCR context.".to_string()],
            visible_windows: Vec::new(),
            ocr_preview: snapshot.likely_targets.first().map(|target| target.label.clone()),
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: estimate_attention_snapshot_storage_bytes(snapshot),
            storage_exact: true,
            row_count: 1,
            file_count: 0,
            has_detail_view: true,
            tags: vec!["attention".to_string(), "snapshot".to_string()],
        });
    }

    slices.sort_by_key(|slice| slice.start_timestamp);

    TimelineRailDto {
        id: "attention".to_string(),
        label: "Gaze / Attention".to_string(),
        description: "Approximate webcam-based attention mapped onto OCR blocks, text spans, and higher-level context surfaces.".to_string(),
        confidence_note: "This rail estimates likely attention targets with uncertainty. It is not exact word-level eye tracking.".to_string(),
        slices,
    }
}

fn estimate_attention_snapshot_storage_bytes(snapshot: &gaze::AttentionSnapshotDto) -> u64 {
    serde_json::to_vec(snapshot)
        .map(|bytes| bytes.len() as u64)
        .unwrap_or(512)
}

fn estimate_attention_span_storage_bytes(span: &gaze::AttentionSpanDto) -> u64 {
    serde_json::to_vec(span)
        .map(|bytes| bytes.len() as u64)
        .unwrap_or(384)
}
