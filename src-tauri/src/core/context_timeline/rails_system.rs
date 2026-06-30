fn build_system_rail(
    sessions: &[SessionRow],
    events: &[ContextEventRow],
    end_timestamp: i64,
) -> TimelineRailDto {
    let mut slices = Vec::new();

    for session in sessions {
        slices.push(ContextSlice {
            id: format!("session-start-{}", session.id),
            rail: "system".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: session.start_timestamp,
            end_timestamp: (session.start_timestamp + 5_000).min(end_timestamp),
            title: "Capture session started".to_string(),
            subtitle: Some(session.id.clone()),
            source: "session_manager".to_string(),
            confidence: 1.0,
            session_id: Some(session.id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec!["A SOURCE desktop capture session was created.".to_string()],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: session_storage_bytes(session),
            storage_exact: true,
            row_count: 1,
            file_count: 0,
            has_detail_view: true,
            tags: vec!["session".to_string()],
        });

        if let Some(end) = session.end_timestamp {
            slices.push(ContextSlice {
                id: format!("session-end-{}", session.id),
                rail: "system".to_string(),
                slice_kind: "event".to_string(),
                start_timestamp: end,
                end_timestamp: end + 5_000,
                title: "Capture session ended".to_string(),
                subtitle: Some(session.id.clone()),
                source: "session_manager".to_string(),
                confidence: 1.0,
                session_id: Some(session.id.clone()),
                app_name: None,
                window_title: None,
                interaction_state: None,
                reasons: vec!["The current SOURCE capture session was stopped.".to_string()],
                visible_windows: Vec::new(),
                ocr_preview: None,
                pii_count: 0,
                evidence_frame_path: None,
                storage_bytes: session_storage_bytes(session),
                storage_exact: true,
                row_count: 1,
                file_count: 0,
                has_detail_view: true,
                tags: vec!["session".to_string()],
            });
        }
    }

    for event in events.iter().filter(|event| event.channel == "system") {
        slices.push(event_row_to_slice(event));
    }

    slices.sort_by_key(|slice| slice.start_timestamp);

    TimelineRailDto {
        id: "system".to_string(),
        label: "System".to_string(),
        description: "Capture lifecycle, app launches/quits, and other desktop session transitions that SOURCE can detect today.".to_string(),
        confidence_note: "Mission Control/App Expose and sleep/wake are shown only when explicitly detected. Absence is not faked.".to_string(),
        slices,
    }
}

fn session_end_lookup(sessions: &[SessionRow]) -> HashMap<String, i64> {
    sessions
        .iter()
        .filter_map(|session| session.end_timestamp.map(|end| (session.id.clone(), end)))
        .collect()
}

fn snapshot_span_end(
    snapshot: &WindowSnapshotRow,
    next_timestamp: Option<i64>,
    session_ends: &HashMap<String, i64>,
    fallback_end_timestamp: i64,
) -> i64 {
    let session_end = snapshot
        .session_id
        .as_ref()
        .and_then(|session_id| session_ends.get(session_id).copied());

    match (next_timestamp, session_end) {
        (Some(next), Some(end)) => next.min(end),
        (Some(next), None) => next,
        (None, Some(end)) => end,
        (None, None) => fallback_end_timestamp,
    }
}

fn event_row_to_slice(row: &ContextEventRow) -> ContextSlice {
    let payload = row
        .payload_json
        .as_ref()
        .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok());
    let title = payload
        .as_ref()
        .and_then(|value| {
            value
                .get("title")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| row.event_type.replace('_', " "));
    let subtitle = payload.as_ref().and_then(|value| {
        value
            .get("subtitle")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
    });

    ContextSlice {
        id: row.id.clone(),
        rail: row.channel.clone(),
        slice_kind: "event".to_string(),
        start_timestamp: row.timestamp,
        end_timestamp: row.timestamp + 5_000,
        title,
        subtitle,
        source: row.source.clone(),
        confidence: row.confidence as f32,
        session_id: row.session_id.clone(),
        app_name: payload.as_ref().and_then(|value| {
            value
                .get("app_name")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        }),
        window_title: payload.as_ref().and_then(|value| {
            value
                .get("window_title")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        }),
        interaction_state: None,
        reasons: vec!["Persisted as an explicit desktop context event.".to_string()],
        visible_windows: Vec::new(),
        ocr_preview: None,
        pii_count: 0,
        evidence_frame_path: None,
        storage_bytes: context_event_storage_bytes(row),
        storage_exact: true,
        row_count: 1,
        file_count: 0,
        has_detail_view: true,
        tags: vec![row.channel.clone(), row.event_type.clone()],
    }
}

