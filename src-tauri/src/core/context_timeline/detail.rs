#[derive(Default)]
struct SliceDetailPayloadState {
    raw_payloads: Vec<RawPayloadDto>,
    pii_entities: Vec<PiiEntityDto>,
    linked_file_paths: Vec<String>,
    ocr_reconstruction: Option<OcrReconstructionDto>,
}

struct SliceQueryData {
    snapshots: Vec<WindowSnapshotRow>,
    ocr_rows: Vec<OcrRow>,
    keyboard: Vec<KeyboardEventSummaryRow>,
    mouse: Vec<MouseEventSummaryRow>,
    frames: Vec<FrameRow>,
    events: Vec<ContextEventRow>,
    sessions: Vec<SessionRow>,
    context: InferredAppContext,
    nearby_system_events: Vec<ContextSlice>,
    visible_windows: Vec<WindowSnapshotDto>,
}

fn detail_rail_label(rail_id: &str) -> &str {
    match rail_id {
        "audio_dictation" => "Dictation",
        "audio_ambient_speech" => "Ambient Audio",
        "audio_sound_events" => "Sound Events",
        "audio_mobile" => "Source Mobile",
        "ocr" => "OCR / Text",
        "keyboard" => "Keyboard Activity",
        "mouse" => "Mouse Activity",
        "focus" => "Focus",
        "visible_windows" => "Running Apps",
        "system" => "System",
        "vision" => "Vision / Scene",
        "attention" => "Gaze / Attention",
        "evidence" => "Screen Evidence",
        _ => "Timeline",
    }
}

async fn hydrate_selected_slice(
    db: &Arc<Database>,
    slice: &mut ContextSlice,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if slice.rail == "ocr" {
        if let Some(scene) = ocr_agent_context::get_scene_snapshot(db, &slice.id).await? {
            slice.ocr_preview = Some(scene.full_text.clone());
            slice.pii_count = scene.pii_entities.len();
            slice.storage_bytes = scene_snapshot_storage_bytes(&scene);
            slice.storage_exact = true;
            slice.row_count = scene.raw_source.raw_row_ids.len() as u64;
            slice.evidence_frame_path = existing_frame_path(&scene.frame_path);
            slice.file_count = u64::from(slice.evidence_frame_path.is_some());
        }
    } else if slice.rail == "audio_dictation" || slice.rail == "audio_mobile" {
        let segments = multimodal::get_asr_segments(
            db,
            slice.start_timestamp,
            slice.end_timestamp,
            None,
        )
        .await?;
        if let Some(segment) = segments.iter().find(|segment| segment.asr_segment_id == slice.id) {
            slice.title = preview_text(&segment.transcript);
            slice.ocr_preview = Some(segment.transcript.clone());
            slice.end_timestamp = segment.end_timestamp.max(segment.start_timestamp + 1);
            slice.storage_bytes = estimate_asr_segment_storage_bytes(segment);
            slice.storage_exact = true;
            slice.tags.retain(|tag| tag != "live" && tag != "final");
            slice.tags.push(if segment.is_final { "final" } else { "live" }.to_string());
        }
    } else if slice.rail == "audio_ambient_speech" {
        let segments = multimodal::get_asr_segments(
            db,
            slice.start_timestamp,
            slice.end_timestamp,
            None,
        )
        .await?;
        let relevant = segments
            .iter()
            .filter(|segment| {
                segment.session_id == slice.session_id.as_deref().unwrap_or("")
                    && segment.source_id != DICTATION_SOURCE_ID
                    && segment.source_id != MOBILE_SOURCE_ID
                    && segment.start_timestamp < slice.end_timestamp
                    && segment.end_timestamp > slice.start_timestamp
            })
            .collect::<Vec<_>>();
        let transcripts = relevant
            .iter()
            .map(|segment| segment.transcript.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !transcripts.is_empty() {
            slice.subtitle = Some(preview_text(&transcripts));
            slice.ocr_preview = Some(transcripts);
            slice.tags.push("asr".to_string());
            slice.tags.push(if relevant.iter().all(|segment| segment.is_final) {
                "final"
            } else {
                "live"
            }.to_string());
        }
        slice.storage_bytes = slice.row_count * 256;
    }
    Ok(())
}

async fn load_slice_query_data(
    db: &Arc<Database>,
    slice: &ContextSlice,
) -> Result<SliceQueryData, Box<dyn std::error::Error + Send + Sync>> {
    if slice.rail.starts_with("audio_") {
        return Ok(SliceQueryData {
            snapshots: Vec::new(),
            ocr_rows: Vec::new(),
            keyboard: Vec::new(),
            mouse: Vec::new(),
            frames: Vec::new(),
            events: Vec::new(),
            sessions: Vec::new(),
            context: InferredAppContext {
                app_name: None,
                bundle_id: None,
                window_title: None,
            },
            nearby_system_events: Vec::new(),
            visible_windows: Vec::new(),
        });
    }
    let snapshots = get_window_snapshots(
        db,
        slice.start_timestamp - 60_000,
        slice.end_timestamp + 60_000,
    )
    .await?;
    let ocr_rows = get_ocr_rows(
        db,
        slice.start_timestamp - 60_000,
        slice.end_timestamp + 60_000,
    )
    .await?;
    let keyboard = get_keyboard_events(
        db,
        slice.start_timestamp - 30_000,
        slice.end_timestamp + 30_000,
    )
    .await?;
    let mouse = get_mouse_events(
        db,
        slice.start_timestamp - 30_000,
        slice.end_timestamp + 30_000,
    )
    .await?;
    let frames = get_frame_rows(
        db,
        slice.start_timestamp - 60_000,
        slice.end_timestamp + 60_000,
    )
    .await?;
    let events = get_context_events(
        db,
        slice.start_timestamp - 120_000,
        slice.end_timestamp + 120_000,
    )
    .await?;
    let sessions = get_sessions(
        db,
        slice.start_timestamp - 120_000,
        slice.end_timestamp + 120_000,
    )
    .await?;
    let context = infer_app_context(db, slice.start_timestamp, slice.session_id.as_deref()).await?;

    let nearby_system_events = events
        .iter()
        .filter(|row| {
            row.channel == "system" && (row.timestamp - slice.start_timestamp).abs() <= 120_000
        })
        .map(event_row_to_slice)
        .collect::<Vec<_>>();

    let visible_windows = if slice.visible_windows.is_empty() {
        snapshots
            .iter()
            .min_by_key(|row| (row.timestamp - slice.start_timestamp).abs())
            .map(parse_visible_windows)
            .transpose()?
            .unwrap_or_default()
    } else {
        slice.visible_windows.clone()
    };

    Ok(SliceQueryData {
        snapshots,
        ocr_rows,
        keyboard,
        mouse,
        frames,
        events,
        sessions,
        context,
        nearby_system_events,
        visible_windows,
    })
}

pub async fn get_context_slice_detail(
    db: &Arc<Database>,
    mut slice: ContextSlice,
) -> Result<ContextSliceDetailDto, Box<dyn std::error::Error + Send + Sync>> {
    if slice.tags.iter().any(|tag| tag == "overview") {
        return overview_slice_detail(db, slice).await;
    }
    hydrate_selected_slice(db, &mut slice).await?;
    let data = load_slice_query_data(db, &slice).await?;
    let mut state = SliceDetailPayloadState::default();
    populate_slice_detail_payloads(db, &slice.rail, &slice.id, &slice, &data, &mut state).await?;

    if state.linked_file_paths.is_empty() {
        if let Some(path) = slice.evidence_frame_path.clone() {
            state.linked_file_paths.push(path);
        }
    }
    state
        .linked_file_paths
        .retain(|path| Path::new(path).exists());

    Ok(ContextSliceDetailDto {
        rail_label: detail_rail_label(&slice.rail).to_string(),
        occurred_at: slice.start_timestamp,
        duration_ms: (slice.end_timestamp - slice.start_timestamp).max(1),
        storage_bytes: slice.storage_bytes,
        storage_exact: slice.storage_exact,
        row_count: slice.row_count,
        file_count: slice.file_count,
        linked_file_paths: state.linked_file_paths,
        focused_app: data.context.app_name.clone().or_else(|| slice.app_name.clone()),
        focused_bundle_id: data.context.bundle_id.clone(),
        visible_windows: data.visible_windows,
        interaction_reasons: slice.reasons.clone(),
        pii_entities: state.pii_entities,
        raw_payloads: state.raw_payloads,
        ocr_reconstruction: state.ocr_reconstruction,
        nearby_system_events: data.nearby_system_events,
        slice,
    })
}
