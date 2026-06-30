fn agent_pii_entity_to_dto(
    entity: &ocr_agent_context::AgentPiiEntityDto,
    timestamp: i64,
    app_name: Option<&str>,
    window_title: Option<&str>,
    frame_path: Option<&str>,
    id_suffix: usize,
) -> PiiEntityDto {
    PiiEntityDto {
        id: format!(
            "scene-pii-{}-{}-{}",
            entity.entity_type, timestamp, id_suffix
        ),
        timestamp,
        app_name: app_name.map(str::to_string),
        window_title: window_title.map(str::to_string),
        entity_type: entity.entity_type.clone(),
        redacted_preview: entity.redacted_preview.clone(),
        confidence: entity.confidence,
        context_text: entity.context_text.clone(),
        bounding_box: entity
            .bounding_box
            .as_ref()
            .and_then(|bbox| serde_json::to_value(bbox).ok()),
        frame_path: frame_path.map(str::to_string),
    }
}


fn take_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn preview_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized_len = normalized.chars().count();
    if normalized_len <= 72 {
        normalized
    } else {
        format!("{}...", take_chars(&normalized, 72))
    }
}

fn redact_match(value: &str) -> String {
    if value.chars().count() <= 4 {
        "••••".to_string()
    } else {
        format!("{}••••", take_chars(value, 4))
    }
}

fn detect_pii_types_in_text(text: &str) -> Vec<String> {
    detect_pii_spans(text)
        .into_iter()
        .map(|(entity_type, _)| entity_type)
        .collect()
}

fn detect_pii_spans(text: &str) -> Vec<(String, String)> {
    let patterns = vec![
        (
            "email",
            Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").unwrap(),
        ),
        (
            "phone",
            Regex::new(r"\b(?:\+?\d{1,3}[-.\s]?)?(?:\(?\d{3}\)?[-.\s]?){1}\d{3}[-.\s]?\d{4}\b")
                .unwrap(),
        ),
        (
            "government_id",
            Regex::new(r"\b\d{3}[- ]?\d{2}[- ]?\d{4}\b").unwrap(),
        ),
        (
            "credit_card",
            Regex::new(r"\b(?:\d[ -]*?){13,19}\b").unwrap(),
        ),
        (
            "ip_address",
            Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap(),
        ),
    ];

    let mut matches = Vec::new();
    for (entity_type, regex) in patterns {
        for capture in regex.find_iter(text) {
            matches.push((entity_type.to_string(), capture.as_str().to_string()));
        }
    }
    matches
}

fn detect_pii_entities(row: &OcrRow, context: &InferredAppContext) -> Vec<PiiEntityDto> {
    let bbox = serde_json::from_str::<serde_json::Value>(&row.bounding_box).ok();
    detect_pii_spans(&row.text)
        .into_iter()
        .enumerate()
        .map(|(index, (entity_type, matched))| PiiEntityDto {
            id: format!("{}-{}-{}", row.id, entity_type, index),
            timestamp: row.timestamp,
            app_name: context.app_name.clone(),
            window_title: context.window_title.clone(),
            entity_type,
            redacted_preview: redact_match(&matched),
            confidence: 0.72,
            context_text: row.text.clone(),
            bounding_box: bbox.clone(),
            frame_path: row.frame_path.clone(),
        })
        .collect()
}
