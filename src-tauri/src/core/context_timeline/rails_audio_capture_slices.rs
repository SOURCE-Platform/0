/// Ambient speech transcripts overlapping a capture span, oldest first.
/// Dictation and mobile segments belong to other rails and are excluded.
fn ambient_transcripts_for_span<'a>(
    span: &AmbientCaptureSpan,
    asr_segments: &'a [AsrSegmentDto],
) -> Vec<&'a AsrSegmentDto> {
    let mut transcripts = asr_segments
        .iter()
        .filter(|segment| {
            segment.source_id != DICTATION_SOURCE_ID
                && segment.source_id != MOBILE_SOURCE_ID
                && segment.session_id == span.session_id
                && segment.start_timestamp < span.end_timestamp
                && segment.end_timestamp > span.start_timestamp
                && !segment.transcript.trim().is_empty()
        })
        .collect::<Vec<_>>();
    transcripts.sort_by_key(|segment| segment.start_timestamp);
    transcripts
}

fn ambient_capture_slice(span: AmbientCaptureSpan, transcripts: &[&AsrSegmentDto]) -> ContextSlice {
    let joined = transcripts
        .iter()
        .map(|segment| segment.transcript.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut tags = vec![
        "audio".to_string(),
        "ambient".to_string(),
        "capture".to_string(),
    ];
    if !joined.is_empty() {
        tags.push("asr".to_string());
        tags.push(
            if transcripts.iter().all(|segment| segment.is_final) {
                "final"
            } else {
                "live"
            }
            .to_string(),
        );
    }
    ContextSlice {
        id: format!(
            "ambient-capture-{}-{}",
            span.session_id, span.start_timestamp
        ),
        rail: "audio_ambient_speech".to_string(),
        slice_kind: "span".to_string(),
        start_timestamp: span.start_timestamp,
        end_timestamp: span.end_timestamp,
        title: "Ambient audio".to_string(),
        subtitle: Some(if joined.is_empty() {
            "Microphone available".to_string()
        } else {
            preview_text(&joined)
        }),
        source: span.source_id,
        confidence: 1.0,
        session_id: Some(span.session_id),
        app_name: None,
        window_title: None,
        interaction_state: None,
        reasons: vec![
            "Joined from back-to-back 2-second mic recordings. Time spent in Right Option dictation is cut out of this block."
                .to_string(),
        ],
        visible_windows: Vec::new(),
        ocr_preview: (!joined.is_empty()).then_some(joined),
        pii_count: 0,
        evidence_frame_path: None,
        storage_bytes: span.chunk_count * 256,
        storage_exact: false,
        row_count: span.chunk_count,
        file_count: 0,
        has_detail_view: true,
        tags,
    }
}

#[cfg(test)]
mod ambient_slice_tests {
    use super::*;

    fn chunk(start: i64, end: i64) -> AmbientCaptureChunk {
        AmbientCaptureChunk {
            session_id: "session".to_string(),
            source_id: "microphone-name:Test Mic".to_string(),
            start_timestamp: start,
            end_timestamp: end,
        }
    }

    fn ambient_transcript(id: &str, start: i64, end: i64, transcript: &str) -> AsrSegmentDto {
        AsrSegmentDto {
            asr_segment_id: id.to_string(),
            session_id: "session".to_string(),
            source_id: "microphone-name:Test Mic".to_string(),
            start_timestamp: start,
            end_timestamp: end,
            language: Some("en".to_string()),
            transcript: transcript.to_string(),
            confidence: Some(0.9),
            model_name: "test".to_string(),
            model_version: "test".to_string(),
            audio_chunk_ids: Vec::new(),
            is_final: true,
        }
    }

    fn dictation_segment(start: i64, end: i64) -> AsrSegmentDto {
        let mut segment = ambient_transcript("dictation", start, end, "test");
        segment.source_id = DICTATION_SOURCE_ID.to_string();
        segment
    }

    #[test]
    fn overlapping_ambient_speech_surfaces_as_transcript() {
        let span = coalesce_ambient_chunks(&[chunk(1_000, 5_000)]).remove(0);
        let segments = vec![
            ambient_transcript("a1", 1_500, 2_500, "hello there"),
            dictation_segment(2_000, 3_000),
        ];
        let transcripts = ambient_transcripts_for_span(&span, &segments);
        assert_eq!(transcripts.len(), 1);
        assert_eq!(transcripts[0].asr_segment_id, "a1");
        let slice = ambient_capture_slice(span, &transcripts);
        assert!(slice.tags.contains(&"asr".to_string()));
        assert!(slice.tags.contains(&"final".to_string()));
        assert_eq!(slice.ocr_preview.as_deref(), Some("hello there"));
        assert_eq!(slice.subtitle.as_deref(), Some("hello there"));
    }

    #[test]
    fn silent_span_keeps_microphone_available_without_transcript_tab() {
        let span = coalesce_ambient_chunks(&[chunk(1_000, 5_000)]).remove(0);
        let transcripts = ambient_transcripts_for_span(&span, &[]);
        let slice = ambient_capture_slice(span, &transcripts);
        assert!(!slice.tags.contains(&"asr".to_string()));
        assert_eq!(slice.ocr_preview, None);
        assert_eq!(
            slice.subtitle.as_deref(),
            Some("Microphone available")
        );
    }
}
