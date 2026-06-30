async fn populate_slice_detail_payloads(
    db: &Arc<Database>,
    rail_id: &str,
    slice_id: &str,
    slice: &ContextSlice,
    data: &SliceQueryData,
    state: &mut SliceDetailPayloadState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match rail_id {
        "system" => collect_system_detail_payloads(slice_id, &data.events, &data.sessions, state),
        "focus" => collect_focus_detail_payloads(slice, &data.snapshots, state),
        "visible_windows" => {
            collect_visible_windows_detail_payloads(slice_id, &data.snapshots, state)
        }
        "interaction" => collect_interaction_detail_payloads(
            slice,
            &data.keyboard,
            &data.mouse,
            &data.ocr_rows,
            state,
        ),
        "ocr" => collect_ocr_detail_payloads(db, slice_id, &data.ocr_rows, state).await?,
        "vision" => collect_vision_detail_payloads(db, slice, slice_id, state).await?,
        "audio" => collect_audio_detail_payloads(db, slice, slice_id, state).await?,
        "attention" => collect_attention_detail_payloads(db, slice, slice_id, state).await?,
        "evidence" => collect_evidence_detail_payloads(slice_id, &data.frames, state),
        _ => {}
    }

    Ok(())
}

fn collect_system_detail_payloads(
    slice_id: &str,
    events: &[ContextEventRow],
    sessions: &[SessionRow],
    state: &mut SliceDetailPayloadState,
) {
    if let Some(session_id) = slice_id
        .strip_prefix("session-start-")
        .or_else(|| slice_id.strip_prefix("session-end-"))
    {
        if let Some(session) = sessions.iter().find(|session| session.id == session_id) {
            state.raw_payloads.push(RawPayloadDto {
                label: "Session row".to_string(),
                raw_json: pretty_json(serde_json::json!({
                    "id": session.id,
                    "start_timestamp": session.start_timestamp,
                    "end_timestamp": session.end_timestamp,
                })),
            });
        }
    } else if let Some(event) = events.iter().find(|row| row.id == slice_id) {
        state.raw_payloads.push(RawPayloadDto {
            label: "Context event".to_string(),
            raw_json: pretty_json(serde_json::json!({
                "id": event.id,
                "session_id": event.session_id,
                "timestamp": event.timestamp,
                "channel": event.channel,
                "event_type": event.event_type,
                "source": event.source,
                "confidence": event.confidence,
                "payload_json": event
                    .payload_json
                    .as_ref()
                    .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
                    .unwrap_or(serde_json::Value::Null),
            })),
        });
    }
}

fn collect_focus_detail_payloads(
    slice: &ContextSlice,
    snapshots: &[WindowSnapshotRow],
    state: &mut SliceDetailPayloadState,
) {
    let matching = snapshots
        .iter()
        .filter(|row| {
            row.timestamp >= slice.start_timestamp
                && row.timestamp <= slice.end_timestamp
                && row.frontmost_app_name == slice.app_name
        })
        .collect::<Vec<_>>();

    for (index, row) in matching.iter().enumerate() {
        state.raw_payloads.push(RawPayloadDto {
            label: format!("Snapshot {}", index + 1),
            raw_json: pretty_json(serde_json::json!({
                "id": row.id,
                "session_id": row.session_id,
                "timestamp": row.timestamp,
                "frontmost_app_name": row.frontmost_app_name,
                "frontmost_bundle_id": row.frontmost_bundle_id,
                "visible_windows": serde_json::from_str::<serde_json::Value>(&row.visible_windows_json)
                    .unwrap_or(serde_json::Value::String(row.visible_windows_json.clone())),
                "confidence": row.confidence,
                "source": row.source,
            })),
        });
    }
}

fn collect_visible_windows_detail_payloads(
    slice_id: &str,
    snapshots: &[WindowSnapshotRow],
    state: &mut SliceDetailPayloadState,
) {
    if let Some(snapshot_id) = slice_id.strip_prefix("visible-") {
        if let Some(row) = snapshots.iter().find(|row| row.id == snapshot_id) {
            state.raw_payloads.push(RawPayloadDto {
                label: "Window snapshot".to_string(),
                raw_json: pretty_json(serde_json::json!({
                    "id": row.id,
                    "session_id": row.session_id,
                    "timestamp": row.timestamp,
                    "frontmost_app_name": row.frontmost_app_name,
                    "frontmost_bundle_id": row.frontmost_bundle_id,
                    "visible_windows": serde_json::from_str::<serde_json::Value>(&row.visible_windows_json)
                        .unwrap_or(serde_json::Value::String(row.visible_windows_json.clone())),
                    "confidence": row.confidence,
                    "source": row.source,
                })),
            });
        }
    }
}

fn collect_interaction_detail_payloads(
    slice: &ContextSlice,
    keyboard: &[KeyboardEventSummaryRow],
    mouse: &[MouseEventSummaryRow],
    ocr_rows: &[OcrRow],
    state: &mut SliceDetailPayloadState,
) {
    let keyboard_rows = keyboard
        .iter()
        .filter(|row| row.timestamp >= slice.start_timestamp && row.timestamp <= slice.end_timestamp)
        .collect::<Vec<_>>();
    let mouse_rows = mouse
        .iter()
        .filter(|row| row.timestamp >= slice.start_timestamp && row.timestamp <= slice.end_timestamp)
        .collect::<Vec<_>>();
    let ocr_event_rows = ocr_rows
        .iter()
        .filter(|row| row.timestamp >= slice.start_timestamp && row.timestamp <= slice.end_timestamp)
        .collect::<Vec<_>>();

    state.raw_payloads.push(RawPayloadDto {
        label: "Interaction bucket".to_string(),
        raw_json: pretty_json(serde_json::json!({
            "slice_id": slice.id,
            "start_timestamp": slice.start_timestamp,
            "end_timestamp": slice.end_timestamp,
            "interaction_state": slice.interaction_state,
            "reasons": slice.reasons,
            "keyboard_events": keyboard_rows.iter().map(|row| serde_json::json!({
                "timestamp": row.timestamp,
                "app_name": row.app_name,
                "window_title": row.window_title,
            })).collect::<Vec<_>>(),
            "mouse_events": mouse_rows.iter().map(|row| serde_json::json!({
                "timestamp": row.timestamp,
                "app_name": row.app_name,
                "window_title": row.window_title,
            })).collect::<Vec<_>>(),
            "ocr_rows": ocr_event_rows.iter().map(|row| serde_json::json!({
                "id": row.id,
                "timestamp": row.timestamp,
                "text": row.text,
                "confidence": row.confidence,
            })).collect::<Vec<_>>(),
        })),
    });
}
