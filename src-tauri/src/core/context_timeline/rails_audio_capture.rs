const AMBIENT_CAPTURE_MAX_GAP_MS: i64 = 750;

#[derive(Debug, Clone)]
struct AmbientCaptureChunk {
    session_id: String,
    source_id: String,
    start_timestamp: i64,
    end_timestamp: i64,
}

#[derive(Debug, Clone)]
struct AmbientCaptureSpan {
    session_id: String,
    source_id: String,
    start_timestamp: i64,
    end_timestamp: i64,
    chunk_count: u64,
}

#[derive(Debug, Clone)]
struct DictationRange {
    session_id: String,
    start_timestamp: i64,
    end_timestamp: i64,
}

fn build_ambient_audio_rail(
    audio_chunks: &[AmbientCaptureChunk],
    asr_segments: &[AsrSegmentDto],
) -> TimelineRailDto {
    let capture_spans = coalesce_ambient_chunks(audio_chunks);
    let dictation_ranges = coalesce_dictation_ranges(asr_segments);
    let slices = capture_spans
        .iter()
        .flat_map(|span| subtract_dictation_ranges(span, &dictation_ranges))
        .map(|piece| {
            let transcripts = ambient_transcripts_for_span(&piece, asr_segments);
            ambient_capture_slice(piece, &transcripts)
        })
        .collect();

    TimelineRailDto::lane(
        "audio_ambient_speech",
        "Ambient Audio",
        "Continuous microphone capture outside deliberate Right Option dictation.",
        "A gap means dictation owned the microphone or ambient capture was unavailable.",
        slices,
    )
}

fn coalesce_ambient_chunks(
    audio_chunks: &[AmbientCaptureChunk],
) -> Vec<AmbientCaptureSpan> {
    let mut chunks = audio_chunks
        .iter()
        .filter(|chunk| chunk.source_id != DICTATION_SOURCE_ID && chunk.source_id != MOBILE_SOURCE_ID)
        .collect::<Vec<_>>();
    chunks.sort_by_key(|chunk| chunk.start_timestamp);

    let mut spans: Vec<AmbientCaptureSpan> = Vec::new();
    for chunk in chunks {
        let can_merge = spans.last().is_some_and(|span| {
            span.session_id == chunk.session_id
                && chunk.start_timestamp <= span.end_timestamp + AMBIENT_CAPTURE_MAX_GAP_MS
        });
        if can_merge {
            let span = spans.last_mut().expect("checked above");
            span.end_timestamp = span.end_timestamp.max(chunk.end_timestamp);
            span.chunk_count += 1;
            if span.source_id != chunk.source_id {
                span.source_id = "ambient_audio".to_string();
            }
        } else {
            spans.push(AmbientCaptureSpan {
                session_id: chunk.session_id.clone(),
                source_id: chunk.source_id.clone(),
                start_timestamp: chunk.start_timestamp,
                end_timestamp: chunk.end_timestamp.max(chunk.start_timestamp + 1),
                chunk_count: 1,
            });
        }
    }
    spans
}

async fn get_ambient_capture_chunks(
    db: &Arc<Database>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<Vec<AmbientCaptureChunk>, sqlx::Error> {
    sqlx::query_as::<_, (String, String, i64, i64)>(
        "SELECT session_id, source_id, start_timestamp, end_timestamp
         FROM audio_chunks
         WHERE start_timestamp <= ? AND end_timestamp >= ?
         ORDER BY start_timestamp ASC",
    )
    .bind(end_timestamp)
    .bind(start_timestamp)
    .fetch_all(db.pool())
    .await
    .map(|rows| {
        rows.into_iter()
            .map(
                |(session_id, source_id, start_timestamp, end_timestamp)| AmbientCaptureChunk {
                    session_id,
                    source_id,
                    start_timestamp,
                    end_timestamp,
                },
            )
            .collect()
    })
}

fn coalesce_dictation_ranges(asr_segments: &[AsrSegmentDto]) -> Vec<DictationRange> {
    let mut dictations = asr_segments
        .iter()
        .filter(|segment| segment.source_id == DICTATION_SOURCE_ID)
        .map(|segment| DictationRange {
            session_id: segment.session_id.clone(),
            start_timestamp: segment.start_timestamp,
            end_timestamp: segment.end_timestamp.max(segment.start_timestamp + 1),
        })
        .collect::<Vec<_>>();
    dictations.sort_by_key(|range| range.start_timestamp);

    let mut merged: Vec<DictationRange> = Vec::new();
    for range in dictations {
        if let Some(previous) = merged.last_mut() {
            if previous.session_id == range.session_id
                && range.start_timestamp <= previous.end_timestamp
            {
                previous.end_timestamp = previous.end_timestamp.max(range.end_timestamp);
                continue;
            }
        }
        merged.push(range);
    }
    merged
}

fn subtract_dictation_ranges(
    span: &AmbientCaptureSpan,
    dictations: &[DictationRange],
) -> Vec<AmbientCaptureSpan> {
    let mut pieces = vec![span.clone()];
    for dictation in dictations.iter().filter(|range| {
        range.session_id == span.session_id
            && range.start_timestamp < span.end_timestamp
            && range.end_timestamp > span.start_timestamp
    }) {
        let mut next = Vec::new();
        for piece in pieces {
            if dictation.end_timestamp <= piece.start_timestamp
                || dictation.start_timestamp >= piece.end_timestamp
            {
                next.push(piece);
                continue;
            }
            if piece.start_timestamp < dictation.start_timestamp {
                next.push(AmbientCaptureSpan {
                    end_timestamp: dictation.start_timestamp,
                    ..piece.clone()
                });
            }
            if piece.end_timestamp > dictation.end_timestamp {
                next.push(AmbientCaptureSpan {
                    start_timestamp: dictation.end_timestamp,
                    ..piece
                });
            }
        }
        pieces = next;
    }
    pieces
}

#[cfg(test)]
mod ambient_capture_tests {
    use super::*;

    fn chunk(start: i64, end: i64) -> AmbientCaptureChunk {
        AmbientCaptureChunk {
            session_id: "session".to_string(),
            source_id: "microphone:0".to_string(),
            start_timestamp: start,
            end_timestamp: end,
        }
    }

    fn dictation(start: i64, end: i64) -> AsrSegmentDto {
        AsrSegmentDto {
            asr_segment_id: "dictation".to_string(),
            session_id: "session".to_string(),
            source_id: DICTATION_SOURCE_ID.to_string(),
            start_timestamp: start,
            end_timestamp: end,
            language: None,
            transcript: "test".to_string(),
            confidence: None,
            model_name: "test".to_string(),
            model_version: "test".to_string(),
            audio_chunk_ids: Vec::new(),
            is_final: true,
        }
    }

    #[test]
    fn coalesces_chunk_boundaries_into_one_steady_span() {
        let spans = coalesce_ambient_chunks(&[
            chunk(1_000, 3_000),
            chunk(3_150, 5_150),
        ]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start_timestamp, 1_000);
        assert_eq!(spans[0].end_timestamp, 5_150);
        assert_eq!(spans[0].chunk_count, 2);
    }

    #[test]
    fn dictation_cuts_a_non_overlapping_hole_in_ambient_capture() {
        let span = coalesce_ambient_chunks(&[
            chunk(1_000, 3_000),
            chunk(3_100, 5_100),
            chunk(5_200, 7_200),
        ])
        .remove(0);
        let pieces = subtract_dictation_ranges(&span, &[DictationRange {
            session_id: "session".to_string(),
            start_timestamp: 2_500,
            end_timestamp: 5_500,
        }]);
        assert_eq!(pieces.len(), 2);
        assert_eq!((pieces[0].start_timestamp, pieces[0].end_timestamp), (1_000, 2_500));
        assert_eq!((pieces[1].start_timestamp, pieces[1].end_timestamp), (5_500, 7_200));
    }

    #[test]
    fn other_sessions_do_not_cut_the_capture_span() {
        let span = coalesce_ambient_chunks(&[chunk(1_000, 5_000)]).remove(0);
        let mut other = dictation(2_000, 3_000);
        other.session_id = "other".to_string();
        let ranges = coalesce_dictation_ranges(&[other]);
        assert_eq!(subtract_dictation_ranges(&span, &ranges).len(), 1);
    }
}
