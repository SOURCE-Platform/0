fn audio_span_title(label: &str) -> String {
    match label {
        "silent" => "Quiet input".to_string(),
        _ => label.replace('_', " "),
    }
}

fn audio_span_subtitle(span: &AudioStateSpanDto) -> String {
    let duration = format_duration_short(span.duration_ms);
    if span.label == "silent" {
        format!("No speech detected · {duration}")
    } else {
        duration
    }
}

fn audio_span_reason(label: &str) -> String {
    if label == "silent" {
        "SOURCE received microphone audio, but its voice detector did not find speech in this period."
            .to_string()
    } else {
        "Built from local microphone chunks plus VAD smoothing.".to_string()
    }
}
