#[derive(FromRow)]
struct AsrOverviewRow {
    asr_segment_id: String,
    session_id: String,
    source_id: String,
    start_timestamp: i64,
    end_timestamp: i64,
    confidence: Option<f32>,
    is_final: i64,
}

#[derive(FromRow)]
struct OcrOverviewRow {
    scene_id: String,
    session_id: String,
    timestamp: i64,
    frontmost_app_name: Option<String>,
    window_title: Option<String>,
    trigger_reason: String,
    avg_confidence: f32,
    block_count: i64,
}

/// The visible timeline needs block positions and labels, not stored text or
/// any source's raw records. Detail is loaded separately on selection.
pub async fn build_context_timeline_overview(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<ContextTimelineData, Box<dyn std::error::Error + Send + Sync>> {
    let asr_rows = sqlx::query_as::<_, AsrOverviewRow>(
        "SELECT asr_segment_id, session_id, source_id, start_timestamp,
                end_timestamp, confidence, is_final
         FROM asr_segments
         WHERE start_timestamp <= ? AND end_timestamp >= ?
         ORDER BY start_timestamp ASC",
    )
    .bind(end_timestamp)
    .bind(start_timestamp)
    .fetch_all(db.pool())
    .await?;
    let asr_segments = asr_rows
        .into_iter()
        .map(|row| AsrSegmentDto {
            asr_segment_id: row.asr_segment_id,
            session_id: row.session_id,
            source_id: row.source_id,
            start_timestamp: row.start_timestamp,
            end_timestamp: row.end_timestamp,
            language: None,
            transcript: String::new(),
            confidence: row.confidence,
            model_name: String::new(),
            model_version: String::new(),
            audio_chunk_ids: Vec::new(),
            is_final: row.is_final != 0,
        })
        .collect::<Vec<_>>();
    let audio_chunks = get_ambient_capture_chunks(db, start_timestamp, end_timestamp).await?;
    let sound_spans =
        multimodal::get_sound_event_spans(db, start_timestamp, end_timestamp, None).await?;
    let sound_detections =
        multimodal::get_sound_event_detections(db, start_timestamp, end_timestamp, None).await?;
    let mut rails = build_audio_rails(
        &audio_chunks,
        &asr_segments,
        &[],
        &sound_spans,
        &sound_detections,
    );
    for rail in &mut rails {
        for slice in &mut rail.slices {
            if rail.id == "audio_dictation" {
                slice.title = "Dictation".to_string();
                slice.storage_bytes = 0;
                slice.storage_exact = false;
            } else if rail.id == "audio_mobile" {
                slice.title = "Mobile recording".to_string();
                slice.storage_bytes = 0;
                slice.storage_exact = false;
            }
            slice.ocr_preview = None;
        }
    }

    let ocr_rows = sqlx::query_as::<_, OcrOverviewRow>(
        "SELECT scene_id, session_id, timestamp, frontmost_app_name,
                window_title, trigger_reason, avg_confidence, block_count
         FROM ocr_scene_snapshots
         WHERE timestamp >= ? AND timestamp <= ?
         ORDER BY timestamp ASC",
    )
    .bind(start_timestamp)
    .bind(end_timestamp)
    .fetch_all(db.pool())
    .await?;
    let ocr_slices = ocr_rows
        .into_iter()
        .map(|row| ContextSlice {
            id: row.scene_id,
            rail: "ocr".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: row.timestamp,
            end_timestamp: row.timestamp + 5_000,
            title: row
                .frontmost_app_name
                .clone()
                .unwrap_or_else(|| "Unknown app".to_string()),
            subtitle: Some(format!(
                "{} lines · {}",
                row.block_count,
                row.trigger_reason.replace('_', " ")
            )),
            source: "ocr_scene_snapshot".to_string(),
            confidence: row.avg_confidence,
            session_id: Some(row.session_id),
            app_name: row.frontmost_app_name,
            window_title: row.window_title,
            interaction_state: None,
            reasons: Vec::new(),
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: 0,
            storage_exact: false,
            row_count: 0,
            file_count: 0,
            has_detail_view: true,
            tags: vec!["ocr".to_string(), row.trigger_reason],
        })
        .collect::<Vec<_>>();
    let ocr_block_count = ocr_slices.len();
    rails.push(TimelineRailDto::lane(
        "ocr",
        "OCR / Text",
        "Captured text blocks available for review.",
        "Select a block to load its captured text and details.",
        ocr_slices,
    ));
    rails.extend(build_activity_overviews(db, start_timestamp, end_timestamp).await?);
    rails.extend(build_desktop_overviews(db, start_timestamp, end_timestamp).await?);
    Ok(ContextTimelineData {
        start_timestamp,
        end_timestamp,
        now_timestamp: chrono::Utc::now()
            .timestamp_millis()
            .clamp(start_timestamp, end_timestamp.max(start_timestamp)),
        default_visible_window_ms: DEFAULT_VISIBLE_WINDOW_MS,
        summary: TimelineSummaryDto {
            ocr_block_count,
            ..TimelineSummaryDto::default()
        },
        rails,
    })
}
