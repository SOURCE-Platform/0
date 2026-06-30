fn build_ocr_rail(
    ocr_scenes: &[AgentSceneSnapshotDto],
    snapshots: &[WindowSnapshotRow],
) -> TimelineRailDto {
    let slices = ocr_scenes
        .iter()
        .map(|scene| {
            let snapshot = snapshots
                .iter()
                .min_by_key(|snap| (snap.timestamp - scene.timestamp).abs());
            let visible_windows = snapshot
                .map(parse_visible_windows)
                .transpose()
                .unwrap_or_default()
                .unwrap_or_default();
            let storage_bytes = scene_snapshot_storage_bytes(scene);
            ContextSlice {
                id: scene.scene_id.clone(),
                rail: "ocr".to_string(),
                slice_kind: "event".to_string(),
                start_timestamp: scene.timestamp,
                end_timestamp: scene.timestamp + 5_000,
                title: preview_text(&scene.full_text),
                subtitle: Some(format!(
                    "{} text blocks · {}",
                    scene.block_count, scene.trigger_reason
                )),
                source: "ocr_scene_snapshot".to_string(),
                confidence: scene.avg_confidence,
                session_id: Some(scene.session_id.clone()),
                app_name: scene
                    .frontmost_app_name
                    .clone()
                    .or_else(|| snapshot.and_then(|snap| snap.frontmost_app_name.clone())),
                window_title: scene.window_title.clone(),
                interaction_state: None,
                reasons: vec![format!(
                    "OCR captured this scene because {} triggered a new OCR pass.",
                    scene.trigger_reason.replace('_', " ")
                )],
                visible_windows,
                ocr_preview: Some(scene.full_text.clone()),
                pii_count: scene.pii_entities.len(),
                evidence_frame_path: scene.frame_path.clone(),
                storage_bytes,
                storage_exact: true,
                row_count: scene.raw_source.raw_row_ids.len() as u64,
                file_count: scene.frame_path.as_ref().map(|_| 1).unwrap_or(0),
                has_detail_view: true,
                tags: vec!["ocr".to_string(), scene.trigger_reason.clone()],
            }
        })
        .collect();

    TimelineRailDto {
        id: "ocr".to_string(),
        label: "OCR / Text".to_string(),
        description: "Captured text blocks that can later be searched, reviewed, and linked back to app context.".to_string(),
        confidence_note: "If OCR is unavailable or overloaded, this rail will degrade cleanly instead of inventing text.".to_string(),
        slices,
    }
}

fn build_vision_rail(
    visual_scenes: &[VisualSceneSnapshotDto],
    visual_spans: &[VisualStateSpanDto],
) -> TimelineRailDto {
    let mut slices = visual_spans
        .iter()
        .map(|span| ContextSlice {
            id: span.visual_state_span_id.clone(),
            rail: "vision".to_string(),
            slice_kind: "span".to_string(),
            start_timestamp: span.first_seen_at,
            end_timestamp: span.last_seen_at.max(span.first_seen_at + 1),
            title: span.label.replace('_', " "),
            subtitle: Some(format!(
                "{} · {}",
                span.state_type,
                format_duration_short(span.duration_ms)
            )),
            source: span.source_id.clone(),
            confidence: span.avg_confidence,
            session_id: Some(span.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![format!(
                "Derived {} span assembled from nearby visual scene snapshots.",
                span.state_type
            )],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: estimate_visual_span_storage_bytes(span),
            storage_exact: false,
            row_count: span.scene_ids.len() as u64,
            file_count: 0,
            has_detail_view: true,
            tags: vec![
                "vision".to_string(),
                span.state_type.clone(),
                span.label.clone(),
            ],
        })
        .collect::<Vec<_>>();

    for scene in visual_scenes.iter().filter(|scene| {
        matches!(
            scene.trigger_reason.as_str(),
            "posture_change"
                | "motion_spike"
                | "person_entered"
                | "person_left"
                | "object_change"
                | "manual_marker"
        )
    }) {
        slices.push(ContextSlice {
            id: scene.visual_scene_id.clone(),
            rail: "vision".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: scene.timestamp,
            end_timestamp: scene.timestamp + 3_000,
            title: format!(
                "{} · {}",
                scene.posture_label.replace('_', " "),
                scene.motion_label.replace('_', " ")
            ),
            subtitle: Some(scene.trigger_reason.replace('_', " ")),
            source: scene.source_id.clone(),
            confidence: scene.avg_confidence,
            session_id: Some(scene.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![format!(
                "Vision capture fired because {}.",
                scene.trigger_reason.replace('_', " ")
            )],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: scene.frame_path.clone(),
            storage_bytes: estimate_visual_scene_storage_bytes(scene),
            storage_exact: true,
            row_count: scene.detections.len() as u64,
            file_count: scene.frame_path.as_ref().map(|_| 1).unwrap_or(0),
            has_detail_view: true,
            tags: vec![
                "vision".to_string(),
                scene.trigger_reason.clone(),
                scene.presence_label.clone(),
                scene.posture_label.clone(),
                scene.motion_label.clone(),
            ],
        });
    }

    slices.sort_by_key(|slice| slice.start_timestamp);

    TimelineRailDto {
        id: "vision".to_string(),
        label: "Vision / Scene".to_string(),
        description: "Camera-derived presence, posture, motion, and visual scene changes captured in the current session.".to_string(),
        confidence_note: "Pose and motion use local detectors. Object labels degrade cleanly when the YOLO adapter is unavailable.".to_string(),
        slices,
    }
}

fn build_audio_rail(
    audio_spans: &[AudioStateSpanDto],
    asr_segments: &[AsrSegmentDto],
) -> TimelineRailDto {
    let mut slices = audio_spans
        .iter()
        .map(|span| ContextSlice {
            id: span.audio_state_span_id.clone(),
            rail: "audio".to_string(),
            slice_kind: "span".to_string(),
            start_timestamp: span.first_seen_at,
            end_timestamp: span.last_seen_at.max(span.first_seen_at + 1),
            title: span.label.replace('_', " "),
            subtitle: Some(format_duration_short(span.duration_ms)),
            source: span.source_id.clone(),
            confidence: span.avg_confidence,
            session_id: Some(span.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec!["Built from local microphone chunks plus VAD smoothing.".to_string()],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: estimate_audio_span_storage_bytes(span),
            storage_exact: false,
            row_count: span.supporting_audio_chunk_ids.len() as u64,
            file_count: 0,
            has_detail_view: true,
            tags: vec!["audio".to_string(), span.label.clone()],
        })
        .collect::<Vec<_>>();

    for segment in asr_segments {
        slices.push(ContextSlice {
            id: segment.asr_segment_id.clone(),
            rail: "audio".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: segment.start_timestamp,
            end_timestamp: segment.end_timestamp.max(segment.start_timestamp + 1),
            title: preview_text(&segment.transcript),
            subtitle: Some("ASR transcript".to_string()),
            source: segment.source_id.clone(),
            confidence: segment.confidence.unwrap_or(0.72),
            session_id: Some(segment.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![
                "Generated by local Whisper transcription on a finalized speech chunk.".to_string(),
            ],
            visible_windows: Vec::new(),
            ocr_preview: Some(segment.transcript.clone()),
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: estimate_asr_segment_storage_bytes(segment),
            storage_exact: true,
            row_count: 1,
            file_count: 0,
            has_detail_view: true,
            tags: vec!["audio".to_string(), "asr".to_string()],
        });
    }

    slices.sort_by_key(|slice| slice.start_timestamp);

    TimelineRailDto {
        id: "audio".to_string(),
        label: "Audio / Speech".to_string(),
        description: "Microphone-derived speaking spans and transcript events when local ASR is available.".to_string(),
        confidence_note: "Speech state works without transcription. ASR is additive and may be absent while audio state still records.".to_string(),
        slices,
    }
}

fn build_evidence_rail(frames: &[FrameRow]) -> TimelineRailDto {
    let slices = frames
        .iter()
        .map(|row| ContextSlice {
            id: format!("frame-{}-{}", row.session_id, row.timestamp),
            rail: "evidence".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: row.timestamp,
            end_timestamp: row.timestamp + 1_000,
            title: "Retained evidence frame".to_string(),
            subtitle: Some(row.session_id.clone()),
            source: "screen_recorder".to_string(),
            confidence: 1.0,
            session_id: Some(row.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![
                "This is a stored frame anchor that can support playback or OCR review."
                    .to_string(),
            ],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: Some(row.file_path.clone()),
            storage_bytes: file_size(&row.file_path),
            storage_exact: true,
            row_count: 1,
            file_count: 1,
            has_detail_view: true,
            tags: vec!["evidence".to_string()],
        })
        .collect();

    TimelineRailDto {
        id: "evidence".to_string(),
        label: "Evidence".to_string(),
        description: "Retained keyframes and media anchors. In real mode these support review rather than acting as the primary navigation object.".to_string(),
        confidence_note: "Evidence can be absent while OCR, input, and system context still remain available.".to_string(),
        slices,
    }
}
