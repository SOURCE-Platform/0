fn build_audio_rails(
    audio_chunks: &[AmbientCaptureChunk],
    asr_segments: &[AsrSegmentDto],
    speech_emotion_segments: &[SpeechEmotionSegmentDto],
    sound_event_spans: &[SoundEventSpanDto],
    sound_event_detections: &[SoundEventDetectionDto],
) -> Vec<TimelineRailDto> {
    // Ambient capture and transcript semantics stay separate: green shows
    // microphone ownership, while purple remains one block per dictation.
    let dictation_rail = build_dictation_rail(asr_segments);
    let ambient_audio_rail = build_ambient_audio_rail(audio_chunks, asr_segments);
    let _emotion_summary_rail = build_audio_emotion_summary_rail(speech_emotion_segments);
    let _emotion_detail_rails = build_audio_emotion_detail_rails(speech_emotion_segments);
    let sound_events_rail =
        build_audio_sound_events_rail(sound_event_spans, sound_event_detections);
    let mobile_rail = build_mobile_rail(asr_segments);

    vec![dictation_rail, ambient_audio_rail, sound_events_rail, mobile_rail]
}

fn dictation_slice(segment: &AsrSegmentDto) -> ContextSlice {
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
    tags.push("dictation".to_string());
    ContextSlice {
        id: segment.asr_segment_id.clone(),
        rail: "audio_dictation".to_string(),
        slice_kind: "event".to_string(),
        start_timestamp: segment.start_timestamp,
        end_timestamp: segment.end_timestamp.max(segment.start_timestamp + 1),
        title: preview_text(&segment.transcript),
        subtitle: Some("Right Option dictation".to_string()),
        source: segment.source_id.clone(),
        confidence: segment.confidence.unwrap_or(0.72),
        session_id: Some(segment.session_id.clone()),
        app_name: None,
        window_title: None,
        interaction_state: None,
        reasons: vec![
            "Initiated with Right Option and transcribed before insertion.".to_string(),
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
    }
}

fn build_dictation_rail(asr_segments: &[AsrSegmentDto]) -> TimelineRailDto {
    let mut dictations = asr_segments
        .iter()
        .filter(|segment| segment.source_id == DICTATION_SOURCE_ID)
        .map(dictation_slice)
        .collect::<Vec<_>>();
    dictations.sort_by_key(|slice| slice.start_timestamp);

    TimelineRailDto::lane(
        "audio_dictation",
        "Right Option Dictation",
        "Speech deliberately initiated with the Right Option shortcut.",
        "Each block is one finalized dictation that was sent to the focused text field.",
        dictations,
    )
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

#[cfg(test)]
mod audio_transcript_rail_tests {
    use super::*;

    fn segment(id: &str, source_id: &str) -> AsrSegmentDto {
        AsrSegmentDto {
            asr_segment_id: id.to_string(),
            session_id: "session".to_string(),
            source_id: source_id.to_string(),
            start_timestamp: 1_000,
            end_timestamp: 2_000,
            language: Some("en".to_string()),
            transcript: format!("transcript {id}"),
            confidence: Some(0.9),
            model_name: "parakeet".to_string(),
            model_version: "test".to_string(),
            audio_chunk_ids: Vec::new(),
            is_final: true,
        }
    }

    #[test]
    fn exposes_three_independent_audio_lanes_without_waveforms() {
        let segments = vec![
            segment("dictated", DICTATION_SOURCE_ID),
            segment("ambient", "microphone:0"),
            segment("mobile", MOBILE_SOURCE_ID),
        ];
        let chunks = vec![AmbientCaptureChunk {
            session_id: "session".to_string(),
            source_id: "microphone:0".to_string(),
            start_timestamp: 2_500,
            end_timestamp: 4_500,
        }];
        let rails = build_audio_rails(&chunks, &segments, &[], &[], &[]);
        let dictation = &rails[0];
        let ambient = &rails[1];

        assert_eq!(rails.len(), 4);
        assert_eq!(dictation.id, "audio_dictation");
        assert_eq!(ambient.id, "audio_ambient_speech");
        assert_eq!(rails[2].id, "audio_sound_events");
        assert_eq!(rails[3].id, "audio_mobile");
        assert_eq!(dictation.slices[0].id, "dictated");
        assert_eq!(rails[3].slices[0].id, "mobile");
        assert!(ambient.slices[0].id.starts_with("ambient-capture-session-"));
        assert!(dictation.waveform.is_none());
        assert!(ambient.waveform.is_none());
    }
}
