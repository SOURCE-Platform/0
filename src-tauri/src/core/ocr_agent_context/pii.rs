use super::{AgentPiiEntityDto, BoundingBox, BBOX_POSITION_TOLERANCE};
use regex::Regex;
use std::collections::{HashMap, HashSet};

pub(super) fn detect_pii_entities(text: &str, bbox: Option<BoundingBox>) -> Vec<AgentPiiEntityDto> {
    detect_pii_spans(text)
        .into_iter()
        .map(|(entity_type, matched)| AgentPiiEntityDto {
            entity_type,
            redacted_preview: redact_match(&matched),
            confidence: 0.72,
            context_text: text.to_string(),
            bounding_box: bbox.clone(),
        })
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
    ];

    let mut matches = Vec::new();
    for (entity_type, regex) in patterns {
        for capture in regex.find_iter(text) {
            matches.push((entity_type.to_string(), capture.as_str().to_string()));
        }
    }
    matches
}

fn redact_match(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= 4 {
        "••••".to_string()
    } else {
        format!("{}••••", chars.iter().take(4).collect::<String>())
    }
}

pub(super) fn normalize_text(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn bbox_is_continuous(left: &BoundingBox, right: &BoundingBox) -> bool {
    if left.overlaps_with(right) {
        return true;
    }
    let dx = (left.x as i64 - right.x as i64).abs();
    let dy = (left.y as i64 - right.y as i64).abs();
    dx <= BBOX_POSITION_TOLERANCE && dy <= BBOX_POSITION_TOLERANCE
}

pub(super) fn union_bbox(left: &BoundingBox, right: &BoundingBox) -> BoundingBox {
    let min_x = left.x.min(right.x);
    let min_y = left.y.min(right.y);
    let max_x = (left.x + left.width).max(right.x + right.width);
    let max_y = (left.y + left.height).max(right.y + right.height);
    BoundingBox::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

pub(super) fn unique_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            unique.push(value);
        }
    }
    unique
}

pub(super) fn preview_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 96 {
        normalized
    } else {
        format!("{}...", normalized.chars().take(96).collect::<String>())
    }
}

pub(super) fn snippet_for_query(text: &str, query: &str, max_length: usize) -> String {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();
    if let Some(position) = text_lower.find(&query_lower) {
        let start = position.saturating_sub(max_length / 2);
        let end = (position + query.len() + max_length / 2).min(text.len());
        let mut snippet = text
            .chars()
            .skip(start)
            .take(end - start)
            .collect::<String>();
        if start > 0 {
            snippet = format!("...{}", snippet);
        }
        if end < text.len() {
            snippet = format!("{}...", snippet);
        }
        snippet
    } else {
        preview_text(text)
    }
}

pub(super) fn classify_entity_type(
    app_name: Option<&str>,
    window_title: Option<&str>,
) -> &'static str {
    let app = app_name.unwrap_or_default().to_lowercase();
    let title = window_title.unwrap_or_default().to_lowercase();
    if app.contains("chrome")
        || app.contains("safari")
        || app.contains("firefox")
        || app.contains("arc")
        || title.contains("http")
    {
        "web_page"
    } else if app.contains("cursor")
        || app.contains("code")
        || app.contains("xcode")
        || app.contains("windsurf")
    {
        "code_editor_view"
    } else if [
        "slack", "discord", "messages", "telegram", "whatsapp", "claude", "chatgpt", "codex",
    ]
    .iter()
    .any(|needle| app.contains(needle))
    {
        "chat_view"
    } else if ["preview", "word", "pages", "notes", "pdf"]
        .iter()
        .any(|needle| app.contains(needle))
    {
        "document_view"
    } else {
        "unknown_view"
    }
}

pub(super) fn extract_title_hint(text: &str) -> Option<String> {
    let first_line = text.lines().next().unwrap_or("").trim();
    (!first_line.is_empty()).then(|| preview_text(first_line))
}

pub(super) fn dominant_terms_from_texts(texts: &[String]) -> Vec<String> {
    let stop_set = [
        "the", "and", "for", "that", "with", "this", "from", "you", "your", "have", "not", "are",
        "was", "but", "they", "their", "into", "what", "when", "how", "why", "where", "http",
        "https", "www", "com",
    ]
    .into_iter()
    .collect::<HashSet<_>>();
    let mut counts = HashMap::new();

    for text in texts {
        for term in normalize_text(text)
            .split(|character: char| !character.is_alphanumeric())
            .filter(|term| term.len() >= 3)
        {
            if !stop_set.contains(term) {
                *counts.entry(term.to_string()).or_insert(0) += 1;
            }
        }
    }

    let mut terms = counts.into_iter().collect::<Vec<_>>();
    terms.sort_by(|a, b| b.1.cmp(&a.1));
    terms.into_iter().take(12).map(|(term, _)| term).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_text() {
        assert_eq!(normalize_text("Hello   World"), "hello world");
    }

    #[test]
    fn test_bbox_continuity() {
        assert!(bbox_is_continuous(
            &BoundingBox::new(10, 10, 100, 20),
            &BoundingBox::new(20, 15, 100, 20)
        ));
    }

    #[test]
    fn test_classify_entity_type() {
        assert_eq!(
            classify_entity_type(Some("Google Chrome"), None),
            "web_page"
        );
        assert_eq!(
            classify_entity_type(Some("Cursor"), None),
            "code_editor_view"
        );
    }
}
