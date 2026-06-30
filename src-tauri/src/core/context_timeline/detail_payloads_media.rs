async fn collect_ocr_detail_payloads(
    db: &Arc<Database>,
    slice_id: &str,
    ocr_rows: &[OcrRow],
    state: &mut SliceDetailPayloadState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(scene) = ocr_agent_context::get_scene_snapshot(db, slice_id).await? {
        let mut reconstruction_blocks = Vec::new();
        let mut max_width = scene.frame_width.unwrap_or(0);
        let mut max_height = scene.frame_height.unwrap_or(0);

        for block in &scene.text_blocks {
            let bbox = serde_json::to_value(&block.bbox).ok();
            max_width = max_width.max(block.bbox.x + block.bbox.width);
            max_height = max_height.max(block.bbox.y + block.bbox.height);
            reconstruction_blocks.push(OcrReconstructionBlockDto {
                id: block.block_id.clone(),
                text: block.text.clone(),
                confidence: block.confidence,
                bounding_box: bbox,
                pii_entities: Vec::new(),
            });
        }

        if let Some(frame_path) = scene.frame_path.clone() {
            state.linked_file_paths.push(frame_path.clone());
            if let Some((width, height)) = get_frame_dimensions(&frame_path) {
                max_width = width.max(max_width);
                max_height = height.max(max_height);
            }
        }

        state.pii_entities.extend(scene.pii_entities.iter().enumerate().map(|(index, entity)| {
            agent_pii_entity_to_dto(
                entity,
                scene.timestamp,
                scene.frontmost_app_name.as_deref(),
                scene.window_title.as_deref(),
                scene.frame_path.as_deref(),
                index + 1000,
            )
        }));

        state.raw_payloads.push(RawPayloadDto {
            label: "Scene snapshot".to_string(),
            raw_json: pretty_json(serde_json::to_value(&scene)?),
        });

        let related_spans = ocr_agent_context::get_text_spans(
            db,
            scene.timestamp.saturating_sub(5_000),
            scene.timestamp.saturating_add(5_000),
            scene.frontmost_app_name.clone(),
        )
        .await?
        .into_iter()
        .filter(|span| span.scene_ids.iter().any(|scene_id| scene_id == &scene.scene_id))
        .collect::<Vec<_>>();

        if !related_spans.is_empty() {
            state.raw_payloads.push(RawPayloadDto {
                label: "Related text spans".to_string(),
                raw_json: pretty_json(serde_json::to_value(&related_spans)?),
            });
        }

        let related_entities = ocr_agent_context::get_context_entities(
            db,
            scene.timestamp.saturating_sub(120_000),
            scene.timestamp.saturating_add(120_000),
            scene.frontmost_app_name.clone(),
            None,
        )
        .await?
        .into_iter()
        .filter(|entity| entity.scene_ids.iter().any(|scene_id| scene_id == &scene.scene_id))
        .collect::<Vec<_>>();

        if !related_entities.is_empty() {
            state.raw_payloads.push(RawPayloadDto {
                label: "Related context entities".to_string(),
                raw_json: pretty_json(serde_json::to_value(&related_entities)?),
            });
        }

        state.ocr_reconstruction = Some(OcrReconstructionDto {
            width: max_width.max(1280),
            height: max_height.max(720),
            frame_path: scene.frame_path.clone(),
            backdrop_available: scene
                .frame_path
                .as_ref()
                .map(|path| Path::new(path).exists())
                .unwrap_or(false),
            blocks: reconstruction_blocks,
        });
    } else {
        let ocr_groups = group_ocr_rows(ocr_rows);
        if let Some(group) = ocr_groups.iter().find(|group| group.id == slice_id) {
            state.raw_payloads.push(RawPayloadDto {
                label: "OCR event (raw fallback)".to_string(),
                raw_json: pretty_json(serde_json::json!({
                    "id": group.id,
                    "session_id": group.session_id,
                    "timestamp": group.timestamp,
                    "frame_path": group.frame_path,
                    "text_blocks": group.blocks.iter().map(|row| serde_json::json!({
                        "id": row.id,
                        "text": row.text,
                        "confidence": row.confidence,
                        "bounding_box": serde_json::from_str::<serde_json::Value>(&row.bounding_box)
                            .unwrap_or(serde_json::Value::String(row.bounding_box.clone())),
                    })).collect::<Vec<_>>(),
                })),
            });
        }
    }

    Ok(())
}

async fn collect_vision_detail_payloads(
    db: &Arc<Database>,
    slice: &ContextSlice,
    slice_id: &str,
    state: &mut SliceDetailPayloadState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(scene) = multimodal::get_visual_scene_snapshot(db, slice_id).await? {
        if let Some(frame_path) = scene.frame_path.clone() {
            state.linked_file_paths.push(frame_path);
        }
        state.raw_payloads.push(RawPayloadDto {
            label: "Visual scene snapshot".to_string(),
            raw_json: pretty_json(serde_json::to_value(&scene)?),
        });
    } else {
        let spans = multimodal::get_visual_state_spans(
            db,
            slice.start_timestamp.saturating_sub(60_000),
            slice.end_timestamp.saturating_add(60_000),
            None,
        )
        .await?;
        if let Some(span) = spans.into_iter().find(|item| item.visual_state_span_id == slice_id) {
            state.raw_payloads.push(RawPayloadDto {
                label: "Visual state span".to_string(),
                raw_json: pretty_json(serde_json::to_value(&span)?),
            });

            let related_scenes = multimodal::get_visual_scene_snapshots(
                db,
                span.first_seen_at,
                span.last_seen_at,
                Some(span.source_id.clone()),
            )
            .await?;
            if let Some(frame_path) = related_scenes.iter().find_map(|scene| scene.frame_path.clone()) {
                state.linked_file_paths.push(frame_path);
            }
            if !related_scenes.is_empty() {
                state.raw_payloads.push(RawPayloadDto {
                    label: "Related visual scenes".to_string(),
                    raw_json: pretty_json(serde_json::to_value(&related_scenes)?),
                });
            }
        }
    }

    Ok(())
}

async fn collect_audio_detail_payloads(
    db: &Arc<Database>,
    slice: &ContextSlice,
    slice_id: &str,
    state: &mut SliceDetailPayloadState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let asr_segments = multimodal::get_asr_segments(
        db,
        slice.start_timestamp.saturating_sub(60_000),
        slice.end_timestamp.saturating_add(60_000),
        None,
    )
    .await?;
    if let Some(segment) = asr_segments.iter().find(|item| item.asr_segment_id == slice_id) {
        state.raw_payloads.push(RawPayloadDto {
            label: "ASR segment".to_string(),
            raw_json: pretty_json(serde_json::to_value(segment)?),
        });
    } else {
        let spans = multimodal::get_audio_state_spans(
            db,
            slice.start_timestamp.saturating_sub(60_000),
            slice.end_timestamp.saturating_add(60_000),
        )
        .await?;
        if let Some(span) = spans.into_iter().find(|item| item.audio_state_span_id == slice_id) {
            state.raw_payloads.push(RawPayloadDto {
                label: "Audio state span".to_string(),
                raw_json: pretty_json(serde_json::to_value(&span)?),
            });

            let chunks = multimodal::get_audio_chunks(
                db,
                span.first_seen_at,
                span.last_seen_at,
                Some(span.source_id.clone()),
            )
            .await?;
            for path in chunks.iter().filter_map(|chunk| chunk.audio_path.clone()) {
                state.linked_file_paths.push(path);
            }
            if !chunks.is_empty() {
                state.raw_payloads.push(RawPayloadDto {
                    label: "Supporting audio chunks".to_string(),
                    raw_json: pretty_json(serde_json::to_value(&chunks)?),
                });
            }
        }
    }

    Ok(())
}

fn collect_evidence_detail_payloads(
    slice_id: &str,
    frames: &[FrameRow],
    state: &mut SliceDetailPayloadState,
) {
    if let Some(frame) = frames
        .iter()
        .find(|row| format!("frame-{}-{}", row.session_id, row.timestamp) == slice_id)
    {
        state.linked_file_paths.push(frame.file_path.clone());
        state.raw_payloads.push(RawPayloadDto {
            label: "Evidence frame".to_string(),
            raw_json: pretty_json(serde_json::json!({
                "session_id": frame.session_id,
                "timestamp": frame.timestamp,
                "file_path": frame.file_path,
                "file_size_bytes": file_size(&frame.file_path),
            })),
        });
    }
}
