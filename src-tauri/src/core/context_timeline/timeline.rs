pub async fn build_context_timeline(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<ContextTimelineData, Box<dyn std::error::Error + Send + Sync>> {
    let sessions = get_sessions(db, start_timestamp, end_timestamp).await?;
    let events = get_context_events(db, start_timestamp, end_timestamp).await?;
    let snapshots = get_window_snapshots(db, start_timestamp, end_timestamp).await?;
    let keyboard = get_keyboard_events(db, start_timestamp, end_timestamp).await?;
    let mouse = get_mouse_events(db, start_timestamp, end_timestamp).await?;
    let ocr_rows = get_ocr_rows(db, start_timestamp, end_timestamp).await?;
    let frames = get_frame_rows(db, start_timestamp, end_timestamp).await?;
    let ocr_scenes =
        ocr_agent_context::get_scene_snapshots(db, start_timestamp, end_timestamp, None).await?;
    let visual_scenes =
        multimodal::get_visual_scene_snapshots(db, start_timestamp, end_timestamp, None).await?;
    let visual_spans =
        multimodal::get_visual_state_spans(db, start_timestamp, end_timestamp, None).await?;
    let audio_spans = multimodal::get_audio_state_spans(db, start_timestamp, end_timestamp).await?;
    let asr_segments =
        multimodal::get_asr_segments(db, start_timestamp, end_timestamp, None).await?;
    let attention_snapshots =
        gaze::get_attention_snapshots(db, start_timestamp, end_timestamp, None, None).await?;
    let attention_spans =
        gaze::get_attention_spans(db, start_timestamp, end_timestamp, None).await?;

    let focus_rail = build_focus_rail(&snapshots, &sessions, end_timestamp);
    let visible_rail = build_visible_windows_rail(&snapshots, &sessions, end_timestamp);
    let system_rail = build_system_rail(&sessions, &events, end_timestamp);
    let interaction_rail = build_interaction_rail(
        &keyboard,
        &mouse,
        &ocr_rows,
        &snapshots,
        start_timestamp,
        end_timestamp,
    );
    let ocr_rail = build_ocr_rail(&ocr_scenes, &snapshots);
    let vision_rail = build_vision_rail(&visual_scenes, &visual_spans);
    let audio_rail = build_audio_rail(&audio_spans, &asr_segments);
    let attention_rail = build_attention_rail(&attention_snapshots, &attention_spans);
    let evidence_rail = build_evidence_rail(&frames);

    let summary = build_summary(
        &focus_rail,
        &visible_rail,
        &interaction_rail,
        &ocr_rail,
        &evidence_rail,
    );
    let now_timestamp = chrono::Utc::now()
        .timestamp_millis()
        .clamp(start_timestamp, end_timestamp);

    Ok(ContextTimelineData {
        start_timestamp,
        end_timestamp,
        now_timestamp,
        default_visible_window_ms: DEFAULT_VISIBLE_WINDOW_MS,
        summary,
        rails: vec![
            system_rail,
            focus_rail,
            visible_rail,
            interaction_rail,
            ocr_rail,
            vision_rail,
            audio_rail,
            attention_rail,
            evidence_rail,
        ],
    })
}
