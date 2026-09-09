fn mobile_slice(segment: &AsrSegmentDto) -> ContextSlice {
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
    tags.push("mobile".to_string());
    ContextSlice {
        id: segment.asr_segment_id.clone(),
        rail: "audio_mobile".to_string(),
        slice_kind: "event".to_string(),
        start_timestamp: segment.start_timestamp,
        end_timestamp: segment.end_timestamp.max(segment.start_timestamp + 1),
        title: preview_text(&segment.transcript),
        subtitle: Some("Source Mobile".to_string()),
        source: segment.source_id.clone(),
        confidence: segment.confidence.unwrap_or(0.72),
        session_id: Some(segment.session_id.clone()),
        app_name: None,
        window_title: None,
        interaction_state: None,
        reasons: vec!["Recorded on iPhone and transcribed on this Mac.".to_string()],
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

fn build_mobile_rail(asr_segments: &[AsrSegmentDto]) -> TimelineRailDto {
    let mut clips = asr_segments
        .iter()
        .filter(|segment| segment.source_id == MOBILE_SOURCE_ID)
        .map(mobile_slice)
        .collect::<Vec<_>>();
    clips.sort_by_key(|slice| slice.start_timestamp);

    TimelineRailDto::lane(
        "audio_mobile",
        "Source Mobile",
        "Voice recordings captured on iPhone and transcribed locally.",
        "Each block is one mobile clip; partial blocks grow while streaming.",
        clips,
    )
}

#[cfg(test)]
mod mobile_rail_tests {
    use super::*;

    fn segment(id: &str, source_id: &str, is_final: bool) -> AsrSegmentDto {
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
            is_final,
        }
    }

    #[test]
    fn filters_mobile_source_only() {
        let segments = vec![
            segment("m1", MOBILE_SOURCE_ID, false),
            segment("d1", DICTATION_SOURCE_ID, true),
            segment("a1", "microphone:0", true),
        ];
        let rail = build_mobile_rail(&segments);
        assert_eq!(rail.id, "audio_mobile");
        assert_eq!(rail.slices.len(), 1);
        assert_eq!(rail.slices[0].id, "m1");
        assert!(rail.slices[0].tags.contains(&"live".to_string()));
    }

    #[test]
    fn empty_without_mobile_rows() {
        let rail = build_mobile_rail(&[segment("d1", DICTATION_SOURCE_ID, true)]);
        assert!(rail.slices.is_empty());
    }
}
