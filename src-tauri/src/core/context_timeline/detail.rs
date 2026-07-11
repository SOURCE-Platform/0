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

fn find_rail_and_slice<'a>(
    rails: &'a [TimelineRailDto],
    rail_id: &str,
    slice_id: &str,
) -> Option<(&'a TimelineRailDto, &'a ContextSlice)> {
    for rail in rails {
        if rail.id == rail_id {
            if let Some(slice) = rail.slices.iter().find(|slice| slice.id == slice_id) {
                return Some((rail, slice));
            }
        }
        if let Some(found) = find_rail_and_slice(&rail.children, rail_id, slice_id) {
            return Some(found);
        }
    }
    None
}

async fn load_slice_query_data(
    db: &Arc<Database>,
    slice: &ContextSlice,
) -> Result<SliceQueryData, Box<dyn std::error::Error + Send + Sync>> {
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
    slice_id: &str,
    rail_id: &str,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<ContextSliceDetailDto, Box<dyn std::error::Error + Send + Sync>> {
    let timeline = build_context_timeline(db, start_timestamp, end_timestamp).await?;
    let (rail, slice) = find_rail_and_slice(&timeline.rails, rail_id, slice_id)
        .ok_or_else(|| format!("Unknown slice lookup: {rail_id}/{slice_id}"))?;
    let slice = slice.clone();

    let data = load_slice_query_data(db, &slice).await?;
    let mut state = SliceDetailPayloadState::default();
    populate_slice_detail_payloads(db, rail_id, slice_id, &slice, &data, &mut state).await?;

    if state.linked_file_paths.is_empty() {
        if let Some(path) = slice.evidence_frame_path.clone() {
            state.linked_file_paths.push(path);
        }
    }

    Ok(ContextSliceDetailDto {
        rail_label: rail.label.clone(),
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
