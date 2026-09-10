fn build_focus_rail(
    snapshots: &[WindowSnapshotRow],
    sessions: &[SessionRow],
    end_timestamp: i64,
) -> TimelineRailDto {
    let mut slices = Vec::new();
    let mut current: Option<ContextSlice> = None;
    let mut current_last: Option<&WindowSnapshotRow> = None;
    let session_ends = session_end_lookup(sessions);

    for (index, snapshot) in snapshots.iter().enumerate() {
        let Some(app_name) = snapshot.frontmost_app_name.clone() else {
            continue;
        };
        let next_timestamp = snapshots.get(index + 1).map(|row| row.timestamp);
        let span_end = snapshot_span_end(snapshot, next_timestamp, &session_ends, end_timestamp);

        match current.as_mut() {
            Some(active) if active.app_name.as_ref() == Some(&app_name) => {
                active.end_timestamp = span_end;
                active.storage_bytes += window_snapshot_storage_bytes(snapshot);
                active.row_count += 1;
                current_last = Some(snapshot);
            }
            _ => {
                if let Some(previous) = current.take() {
                    slices.push(with_last_visible_windows(previous, current_last));
                }
                current_last = Some(snapshot);
                current = Some(ContextSlice {
                    id: format!("focus-{}", snapshot.id),
                    rail: "focus".to_string(),
                    slice_kind: "span".to_string(),
                    start_timestamp: snapshot.timestamp,
                    end_timestamp: span_end,
                    title: app_name.clone(),
                    subtitle: Some(
                        snapshot
                            .frontmost_bundle_id
                            .clone()
                            .unwrap_or_else(|| "Frontmost app".to_string()),
                    ),
                    source: snapshot.source.clone(),
                    confidence: snapshot.confidence as f32,
                    session_id: snapshot.session_id.clone(),
                    app_name: Some(app_name),
                    window_title: None,
                    interaction_state: None,
                    reasons: vec!["Derived from periodic frontmost-app snapshots.".to_string()],
                    visible_windows: Vec::new(),
                    ocr_preview: None,
                    pii_count: 0,
                    evidence_frame_path: None,
                    storage_bytes: window_snapshot_storage_bytes(snapshot),
                    storage_exact: false,
                    row_count: 1,
                    file_count: 0,
                    has_detail_view: true,
                    tags: vec!["frontmost".to_string()],
                });
            }
        }
    }

    if let Some(active) = current {
        slices.push(with_last_visible_windows(active, current_last));
    }

    TimelineRailDto::lane(
        "focus",
        "Focus",
        "Which app SOURCE believes was frontmost at a given moment.",
        "Focus is based on OS snapshots taken during active capture. Historical gaps are left visible instead of backfilled.",
        slices,
    )
}

/// A focus span shows the windows from its most recent snapshot, so parse that
/// one only. Parsing every snapshot to overwrite the same field re-read the whole
/// day's window lists on every timeline refresh.
fn with_last_visible_windows(
    mut slice: ContextSlice,
    last: Option<&WindowSnapshotRow>,
) -> ContextSlice {
    if let Some(snapshot) = last {
        slice.visible_windows = parse_visible_windows(snapshot).unwrap_or_default();
    }
    slice
}

fn build_visible_windows_rail(
    snapshots: &[WindowSnapshotRow],
    sessions: &[SessionRow],
    end_timestamp: i64,
) -> TimelineRailDto {
    let session_ends = session_end_lookup(sessions);
    let slices = snapshots
        .iter()
        .enumerate()
        .map(|(index, snapshot)| {
            let windows = parse_visible_windows(snapshot).unwrap_or_default();
            let span_end = snapshot_span_end(
                snapshot,
                snapshots
                .get(index + 1)
                .map(|row| row.timestamp),
                &session_ends,
                end_timestamp,
            );
            let frontmost = snapshot
                .frontmost_app_name
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            ContextSlice {
                id: format!("visible-{}", snapshot.id),
                rail: "visible_windows".to_string(),
                slice_kind: "span".to_string(),
                start_timestamp: snapshot.timestamp,
                end_timestamp: span_end,
                title: format!("{} visible apps", windows.len()),
                subtitle: Some(format!("Frontmost: {}", frontmost)),
                source: snapshot.source.clone(),
                confidence: snapshot.confidence as f32,
                session_id: snapshot.session_id.clone(),
                app_name: snapshot.frontmost_app_name.clone(),
                window_title: None,
                interaction_state: None,
                reasons: vec!["Visible-window context is best-effort in v1 and uses running-app snapshots as a proxy.".to_string()],
                visible_windows: windows,
                ocr_preview: None,
                pii_count: 0,
                evidence_frame_path: None,
                storage_bytes: window_snapshot_storage_bytes(snapshot),
                storage_exact: false,
                row_count: 1,
                file_count: 0,
                has_detail_view: true,
                tags: vec!["best_effort".to_string(), "visible".to_string()],
            }
        })
        .collect();

    TimelineRailDto::lane(
        "visible_windows",
        "Visible Windows",
        "Best-effort scene context for what else was on screen alongside the frontmost app.",
        "v1 uses running-app snapshots, not a full historical macOS window graph, so this rail is explicitly low-confidence.",
        slices,
    )
}

fn build_interaction_rail(
    keyboard: &[KeyboardEventSummaryRow],
    mouse: &[MouseEventSummaryRow],
    ocr_rows: &[OcrRow],
    snapshots: &[WindowSnapshotRow],
    start_timestamp: i64,
    end_timestamp: i64,
) -> TimelineRailDto {
    let mut slices = Vec::new();
    let mut bucket_start = start_timestamp;

    while bucket_start < end_timestamp {
        let bucket_end = (bucket_start + DEFAULT_BUCKET_MS).min(end_timestamp);
        let kb: Vec<_> = keyboard
            .iter()
            .filter(|row| row.timestamp >= bucket_start && row.timestamp < bucket_end)
            .collect();
        let ms: Vec<_> = mouse
            .iter()
            .filter(|row| row.timestamp >= bucket_start && row.timestamp < bucket_end)
            .collect();
        let ocr: Vec<_> = ocr_rows
            .iter()
            .filter(|row| row.timestamp >= bucket_start && row.timestamp < bucket_end)
            .collect();
        let snap = snapshots
            .iter()
            .rev()
            .find(|row| row.timestamp >= bucket_start && row.timestamp < bucket_end)
            .cloned();

        if kb.is_empty() && ms.is_empty() && ocr.is_empty() && snap.is_none() {
            bucket_start = bucket_end;
            continue;
        }

        let (state, reasons, confidence) = if !kb.is_empty() && ms.is_empty() {
            (
                "active_typing".to_string(),
                vec!["Keyboard events present".to_string()],
                1.0,
            )
        } else if kb.is_empty() && !ms.is_empty() {
            (
                "active_pointer".to_string(),
                vec!["Mouse movement or click events present".to_string()],
                1.0,
            )
        } else if !kb.is_empty() && !ms.is_empty() {
            (
                "mixed".to_string(),
                vec![
                    "Keyboard events present".to_string(),
                    "Mouse events present".to_string(),
                ],
                1.0,
            )
        } else {
            let text = ocr
                .iter()
                .map(|row| row.text.to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            let voice_inferred = text.contains("recording")
                || text.contains("waveform")
                || text.contains("dictation")
                || text.contains("microphone");
            if voice_inferred {
                (
                    "voice_input_inferred".to_string(),
                    vec!["No keyboard or mouse input was present, but OCR captured recording-like cues.".to_string()],
                    0.55,
                )
            } else {
                (
                    "passive_viewing".to_string(),
                    vec!["No direct input was present, but the screen context was still changing or visible.".to_string()],
                    0.65,
                )
            }
        };

        let app_name = kb
            .first()
            .map(|row| row.app_name.clone())
            .or_else(|| ms.first().map(|row| row.app_name.clone()))
            .or_else(|| snap.as_ref().and_then(|row| row.frontmost_app_name.clone()));
        let window_title = kb
            .first()
            .map(|row| row.window_title.clone())
            .or_else(|| ms.first().map(|row| row.window_title.clone()));
        let storage_bytes = kb
            .iter()
            .map(|row| keyboard_event_storage_bytes(row))
            .sum::<u64>()
            + ms.iter()
                .map(|row| mouse_event_storage_bytes(row))
                .sum::<u64>()
            + ocr
                .iter()
                .map(|row| ocr_row_storage_bytes(row))
                .sum::<u64>();
        let row_count = (kb.len() + ms.len() + ocr.len()) as u64;

        slices.push(ContextSlice {
            id: format!("interaction-{}", bucket_start),
            rail: "interaction".to_string(),
            slice_kind: "span".to_string(),
            start_timestamp: bucket_start,
            end_timestamp: bucket_end,
            title: state.replace('_', " "),
            subtitle: app_name.clone(),
            source: if confidence < 1.0 {
                "hybrid_inference".to_string()
            } else {
                "input_recorder".to_string()
            },
            confidence,
            session_id: None,
            app_name,
            window_title,
            interaction_state: Some(state),
            reasons,
            visible_windows: snap
                .as_ref()
                .map(parse_visible_windows)
                .transpose()
                .unwrap_or_default()
                .unwrap_or_default(),
            ocr_preview: ocr.first().map(|row| preview_text(&row.text)),
            pii_count: ocr
                .iter()
                .map(|row| detect_pii_types_in_text(&row.text).len())
                .sum(),
            evidence_frame_path: None,
            storage_bytes,
            storage_exact: false,
            row_count,
            file_count: 0,
            has_detail_view: true,
            tags: vec!["interaction".to_string()],
        });

        bucket_start = bucket_end;
    }

    TimelineRailDto::lane(
        "interaction",
        "Interaction",
        "Hard input signals when available, plus clearly-labeled inferred activity states when direct integrations do not exist.",
        "Inferred voice input and passive viewing are always labeled as inferences, not authoritative app integrations.",
        slices,
    )
}
