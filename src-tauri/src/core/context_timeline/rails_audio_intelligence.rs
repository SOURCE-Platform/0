fn build_audio_group_rail(
    audio_chunks: &[multimodal::AudioChunkDto],
    audio_spans: &[AudioStateSpanDto],
    asr_segments: &[AsrSegmentDto],
    speech_emotion_segments: &[SpeechEmotionSegmentDto],
    sound_event_spans: &[SoundEventSpanDto],
    sound_event_detections: &[SoundEventDetectionDto],
) -> TimelineRailDto {
    // Audio-only focus: speech transcripts (including dictation) plus
    // environmental sound labels. Emotion rails are parked, not deleted —
    // their builders below stay for the future emotion pass — but they no
    // longer ship in the tree. The models still run at capture time.
    let speech_rail = build_audio_speech_rail(audio_chunks, audio_spans, asr_segments);
    let _emotion_summary_rail = build_audio_emotion_summary_rail(speech_emotion_segments);
    let _emotion_detail_rails = build_audio_emotion_detail_rails(speech_emotion_segments);
    let sound_events_rail = build_audio_sound_events_rail(sound_event_spans, sound_event_detections);

    let mut children = Vec::with_capacity(2);
    children.push(speech_rail);
    children.push(sound_events_rail);

    TimelineRailDto::group(
        "audio",
        "Audio",
        "Speech transcripts and sound-event labels grouped under one audio hierarchy for easier review.",
        "Audio keeps the current local speech and transcript models; emotion rails are parked for a later pass.",
        false,
        children,
    )
}

fn build_audio_speech_rail(
    audio_chunks: &[multimodal::AudioChunkDto],
    audio_spans: &[AudioStateSpanDto],
    asr_segments: &[AsrSegmentDto],
) -> TimelineRailDto {
    let mut slices = Vec::new();
    slices.extend(audio_spans
        .iter()
        .map(|span| ContextSlice {
            id: span.audio_state_span_id.clone(),
            rail: "audio_speech".to_string(),
            slice_kind: "span".to_string(),
            start_timestamp: span.first_seen_at,
            end_timestamp: span.last_seen_at.max(span.first_seen_at + 1),
            title: audio_span_title(&span.label),
            subtitle: Some(audio_span_subtitle(span)),
            source: span.source_id.clone(),
            confidence: span.avg_confidence,
            session_id: Some(span.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![audio_span_reason(&span.label)],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: estimate_audio_span_storage_bytes(span),
            storage_exact: false,
            row_count: span.supporting_audio_chunk_ids.len() as u64,
            file_count: 0,
            has_detail_view: true,
            tags: vec!["audio".to_string(), "speech".to_string(), span.label.clone()],
        })
        .collect::<Vec<_>>());

    for segment in asr_segments {
        // Foreground Right Option dictations read differently from overheard
        // speech: they were deliberately spoken to be typed somewhere.
        let dictated = segment.source_id == DICTATION_SOURCE_ID;
        let mut tags = vec![
            "audio".to_string(),
            "speech".to_string(),
            "asr".to_string(),
            if segment.is_final {
                "final".to_string()
            } else {
                "live".to_string()
            },
        ];
        if dictated {
            tags.push("dictation".to_string());
        }
        slices.push(ContextSlice {
            id: segment.asr_segment_id.clone(),
            rail: "audio_speech".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: segment.start_timestamp,
            end_timestamp: segment.end_timestamp.max(segment.start_timestamp + 1),
            title: preview_text(&segment.transcript),
            subtitle: Some(
                if dictated {
                    "Dictated prompt"
                } else if segment.is_final {
                    "Final transcript"
                } else {
                    "Live transcript"
                }
                .to_string(),
            ),
            source: segment.source_id.clone(),
            confidence: segment.confidence.unwrap_or(0.72),
            session_id: Some(segment.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![
                if segment.is_final {
                    "Finalized by local Parakeet after speech ended."
                } else {
                    "Live local Parakeet transcript while speech continues."
                }
                .to_string(),
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
            tags,
        });
    }

    slices.sort_by_key(|slice| slice.start_timestamp);

    let waveform = build_audio_waveform("microphone:0", audio_chunks)
        .or_else(|| audio_chunks.first().and_then(|chunk| build_audio_waveform(&chunk.source_id, audio_chunks)));
    match waveform {
        Some(waveform) => TimelineRailDto::lane_with_waveform(
            "audio_speech",
            "Speech",
            "Continuous audio envelope with durable speaking spans and transcript markers.",
            "The waveform is stitched from stored samples. Speech state and transcription remain separate overlay data.",
            waveform,
            slices,
        ),
        None => TimelineRailDto::lane(
            "audio_speech",
            "Speech",
            "Continuous audio envelope with durable speaking spans and transcript markers.",
            "Speech state works without transcription. ASR is additive and may be missing while speaking spans still record normally.",
            slices,
        ),
    }
}

fn build_audio_emotion_summary_rail(
    speech_emotion_segments: &[SpeechEmotionSegmentDto],
) -> TimelineRailDto {
    let slices = speech_emotion_segments
        .iter()
        .map(|segment| build_emotion_slice("audio_emotion_summary", segment))
        .collect::<Vec<_>>();

    TimelineRailDto::lane(
        "audio_emotion_summary_lane",
        "Overview",
        "Top-level polarity view of speech emotion with positive, negative, neutral, and uncertain segments aligned to the same timeline.",
        "Positive emotions rise above neutral, negative emotions fall below it, and uncertain segments stay visually quiet in this summary view.",
        slices,
    )
}

fn build_audio_emotion_detail_rails(
    speech_emotion_segments: &[SpeechEmotionSegmentDto],
) -> Vec<TimelineRailDto> {
    AUDIO_EMOTION_RAILS
        .iter()
        .map(|(canonical_label, display_label)| {
            let rail_id = format!("audio_emotion_{canonical_label}");
            let slices = speech_emotion_segments
                .iter()
                .filter(|segment| segment.canonical_label == *canonical_label)
                .map(|segment| build_emotion_slice(&rail_id, segment))
                .collect::<Vec<_>>();

            TimelineRailDto::lane(
                &rail_id,
                display_label,
                &format!(
                    "Detailed {} emotion confidence view built from the existing speech-emotion segments.",
                    display_label.to_lowercase()
                ),
                "Each lane shows only one canonical emotion, with height driven directly by the stored confidence for that segment.",
                slices,
            )
        })
        .collect()
}

fn build_audio_sound_events_rail(
    sound_event_spans: &[SoundEventSpanDto],
    sound_event_detections: &[SoundEventDetectionDto],
) -> TimelineRailDto {
    let mut slices = sound_event_spans
        .iter()
        .map(|span| ContextSlice {
            id: span.sound_event_span_id.clone(),
            rail: "audio_sound_events".to_string(),
            slice_kind: "span".to_string(),
            start_timestamp: span.first_seen_at,
            end_timestamp: span.last_seen_at.max(span.first_seen_at + 1),
            title: span.canonical_label.replace('_', " "),
            subtitle: Some(format_duration_short(span.duration_ms)),
            source: span.source_id.clone(),
            confidence: span.avg_confidence,
            session_id: Some(span.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![
                "Merged from nearby sound-event detections that shared the same canonical label."
                    .to_string(),
            ],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: estimate_sound_event_span_storage_bytes(span),
            storage_exact: false,
            row_count: span.supporting_detection_ids.len() as u64,
            file_count: 0,
            has_detail_view: true,
            tags: vec![
                "audio".to_string(),
                "sound_event".to_string(),
                span.canonical_label.clone(),
            ],
        })
        .collect::<Vec<_>>();

    for detection in sound_event_detections {
        slices.push(ContextSlice {
            id: detection.sound_event_detection_id.clone(),
            rail: "audio_sound_events".to_string(),
            slice_kind: "event".to_string(),
            start_timestamp: detection.start_timestamp,
            end_timestamp: detection.end_timestamp.max(detection.start_timestamp + 1),
            title: detection.canonical_label.replace('_', " "),
            subtitle: Some("Sound event".to_string()),
            source: detection.source_id.clone(),
            confidence: detection.confidence,
            session_id: Some(detection.session_id.clone()),
            app_name: None,
            window_title: None,
            interaction_state: None,
            reasons: vec![format!(
                "Detected by the local sound-event model as {}.",
                detection.event_label
            )],
            visible_windows: Vec::new(),
            ocr_preview: None,
            pii_count: 0,
            evidence_frame_path: None,
            storage_bytes: estimate_sound_event_detection_storage_bytes(detection),
            storage_exact: true,
            row_count: 1,
            file_count: 0,
            has_detail_view: true,
            tags: vec![
                "audio".to_string(),
                "sound_event".to_string(),
                detection.canonical_label.clone(),
            ],
        });
    }

    slices.sort_by_key(|slice| slice.start_timestamp);

    TimelineRailDto::lane(
        "audio_sound_events",
        "Sound Events",
        "Local non-speech audio detections such as impacts, doors, footsteps, vehicles, and other scene sounds.",
        "Sound-event labels are approximate scene-memory cues rather than forensic audio classifications.",
        slices,
    )
}

fn build_emotion_slice(rail_id: &str, segment: &SpeechEmotionSegmentDto) -> ContextSlice {
    let polarity = emotion_polarity(segment.canonical_label.as_str());
    ContextSlice {
        id: segment.speech_emotion_segment_id.clone(),
        rail: rail_id.to_string(),
        slice_kind: "event".to_string(),
        start_timestamp: segment.start_timestamp,
        end_timestamp: segment.end_timestamp.max(segment.start_timestamp + 1),
        title: prettify_emotion_label(&segment.canonical_label),
        subtitle: Some(format!(
            "{} · {}%",
            polarity_label(polarity),
            (segment.confidence * 100.0).round() as i64
        )),
        source: segment.source_id.clone(),
        confidence: segment.confidence,
        session_id: Some(segment.session_id.clone()),
        app_name: None,
        window_title: None,
        interaction_state: None,
        reasons: vec![
            "Inferred from a finalized speech chunk using the local speech-emotion model."
                .to_string(),
        ],
        visible_windows: Vec::new(),
        ocr_preview: None,
        pii_count: 0,
        evidence_frame_path: None,
        storage_bytes: estimate_speech_emotion_storage_bytes(segment),
        storage_exact: true,
        row_count: 1,
        file_count: 0,
        has_detail_view: true,
        tags: vec![
            "audio".to_string(),
            "emotion".to_string(),
            "emotion_summary".to_string(),
            segment.canonical_label.clone(),
            polarity_label(polarity).to_lowercase(),
        ],
    }
}
