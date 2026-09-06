fn build_audio_waveform(
    source_id: &str,
    audio_chunks: &[multimodal::AudioChunkDto],
) -> Option<TimelineWaveformDto> {
    let chunks = audio_chunks
        .iter()
        .filter(|chunk| chunk.source_id == source_id)
        .collect::<Vec<_>>();
    if chunks.is_empty() {
        return None;
    }

    let mut samples = Vec::new();
    for chunk in chunks {
        let duration = (chunk.end_timestamp - chunk.start_timestamp).max(1);
        let count = chunk.waveform_levels.len().max(1) as i64;
        for (index, level) in chunk.waveform_levels.iter().enumerate() {
            samples.push(TimelineWaveformSampleDto {
                timestamp: chunk.start_timestamp + (duration * index as i64 / count),
                level: level.clamp(0.0, 1.0),
            });
        }
    }

    Some(TimelineWaveformDto {
        source_id: source_id.to_string(),
        source_label: audio_source_label(source_id),
        samples,
    })
}

fn audio_source_label(source_id: &str) -> String {
    if source_id.starts_with("microphone:") {
        "Microphone".to_string()
    } else if source_id.starts_with("desktop_app:") {
        source_id
            .trim_start_matches("desktop_app:")
            .replace('.', " ")
    } else {
        "Desktop audio".to_string()
    }
}
