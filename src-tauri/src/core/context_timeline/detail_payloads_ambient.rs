async fn collect_ambient_capture_detail_payloads(
    db: &Arc<Database>,
    slice: &ContextSlice,
    state: &mut SliceDetailPayloadState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut chunks =
        multimodal::get_audio_chunks(db, slice.start_timestamp, slice.end_timestamp, None).await?;
    if let Some(session_id) = slice.session_id.as_deref() {
        chunks.retain(|chunk| chunk.session_id == session_id);
    }
    for path in chunks.iter().filter_map(|chunk| chunk.audio_path.clone()) {
        state.linked_file_paths.push(path);
    }
    let speech_chunks = chunks.iter().filter(|chunk| chunk.speech_detected).count();
    state.raw_payloads.push(RawPayloadDto {
        label: "Ambient capture span".to_string(),
        raw_json: pretty_json(serde_json::json!({
            "slice_id": slice.id,
            "session_id": slice.session_id,
            "source": slice.source,
            "start_timestamp": slice.start_timestamp,
            "end_timestamp": slice.end_timestamp,
            "chunk_count": chunks.len(),
            "speech_chunks": speech_chunks,
            "note": "Green means the mic was on. Only chunks with speech_detected=true were sent to transcription.",
        })),
    });
    if !chunks.is_empty() {
        state.raw_payloads.push(RawPayloadDto {
            label: "Supporting audio chunks".to_string(),
            raw_json: pretty_json(serde_json::to_value(&chunks)?),
        });
    }
    let mut transcripts =
        multimodal::get_asr_segments(db, slice.start_timestamp, slice.end_timestamp, None).await?;
    if let Some(session_id) = slice.session_id.as_deref() {
        transcripts.retain(|segment| segment.session_id == session_id);
    }
    if !transcripts.is_empty() {
        state.raw_payloads.push(RawPayloadDto {
            label: "Supporting transcripts".to_string(),
            raw_json: pretty_json(serde_json::to_value(&transcripts)?),
        });
    }

    Ok(())
}
