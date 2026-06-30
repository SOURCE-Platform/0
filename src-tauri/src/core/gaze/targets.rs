use super::types::AttentionTargetDto;
use crate::core::ocr_agent_context::{
    AgentContextEntityDto, AgentSceneSnapshotDto, AgentTextSpanDto,
};
use crate::models::ocr::BoundingBox;
use std::collections::HashMap;

pub(super) fn resolve_targets(
    scene: Option<&AgentSceneSnapshotDto>,
    text_spans: &[AgentTextSpanDto],
    entities: &[AgentContextEntityDto],
    screen_x: f32,
    screen_y: f32,
    accuracy_radius_px: f32,
    screen_width: f32,
    screen_height: f32,
) -> Vec<AttentionTargetDto> {
    let mut merged = HashMap::<String, AttentionTargetDto>::new();
    if let Some(scene) = scene {
        let frame_width = scene.frame_width.unwrap_or(screen_width as u32).max(1) as f32;
        let frame_height = scene.frame_height.unwrap_or(screen_height as u32).max(1) as f32;

        for block in &scene.text_blocks {
            let bbox = scale_bbox(
                &block.bbox,
                frame_width,
                frame_height,
                screen_width,
                screen_height,
            );
            let distance_px = distance_to_bbox(screen_x, screen_y, &bbox);
            let overlap_score =
                (1.0 - (distance_px / accuracy_radius_px.max(48.0))).clamp(0.0, 1.0);
            if overlap_score <= 0.08 {
                continue;
            }

            let matching_span = text_spans.iter().find(|span| {
                span.scene_ids
                    .iter()
                    .any(|scene_id| scene_id == &scene.scene_id)
                    && normalize_text(&span.canonical_text) == normalize_text(&block.text)
            });
            let (target_type, target_id, label) = if let Some(span) = matching_span {
                (
                    "text_span",
                    span.text_span_id.clone(),
                    preview_text(&span.canonical_text),
                )
            } else {
                (
                    "ocr_block",
                    format!("{}:{}", scene.scene_id, block.block_id),
                    preview_text(&block.text),
                )
            };
            let probability =
                (overlap_score * block.confidence * scene.avg_confidence).clamp(0.05, 0.99);
            merge_target(
                &mut merged,
                target_type,
                target_id,
                label,
                probability,
                distance_px,
                overlap_score,
            );
        }

        if let Some(entity) = entities.iter().find(|item| {
            item.scene_ids
                .iter()
                .any(|scene_id| scene_id == &scene.scene_id)
        }) {
            merge_target(
                &mut merged,
                "context_entity",
                entity.entity_id.clone(),
                preview_text(entity.title_hint.as_deref().unwrap_or(&entity.summary_text)),
                0.34,
                accuracy_radius_px,
                0.28,
            );
        }

        merge_target(
            &mut merged,
            "scene_snapshot",
            scene.scene_id.clone(),
            preview_text(&scene.full_text),
            0.18,
            accuracy_radius_px * 1.5,
            0.12,
        );
    }

    let mut targets = merged.into_values().collect::<Vec<_>>();
    targets.sort_by(|a, b| {
        b.probability
            .total_cmp(&a.probability)
            .then_with(|| a.distance_px.total_cmp(&b.distance_px))
    });
    targets.truncate(8);
    targets
}

fn merge_target(
    merged: &mut HashMap<String, AttentionTargetDto>,
    target_type: &str,
    target_id: String,
    label: String,
    probability: f32,
    distance_px: f32,
    overlap_score: f32,
) {
    let key = format!("{target_type}:{target_id}");
    let entry = merged.entry(key).or_insert(AttentionTargetDto {
        target_type: target_type.to_string(),
        target_id,
        label,
        probability,
        distance_px,
        overlap_score,
    });
    if probability > entry.probability {
        entry.probability = probability;
    }
    if distance_px < entry.distance_px {
        entry.distance_px = distance_px;
    }
    if overlap_score > entry.overlap_score {
        entry.overlap_score = overlap_score;
    }
}

fn scale_bbox(
    bbox: &BoundingBox,
    frame_width: f32,
    frame_height: f32,
    screen_width: f32,
    screen_height: f32,
) -> BoundingBox {
    let scale_x = if frame_width > 0.0 {
        screen_width / frame_width
    } else {
        1.0
    };
    let scale_y = if frame_height > 0.0 {
        screen_height / frame_height
    } else {
        1.0
    };
    BoundingBox::new(
        (bbox.x as f32 * scale_x).round() as u32,
        (bbox.y as f32 * scale_y).round() as u32,
        (bbox.width as f32 * scale_x).round().max(1.0) as u32,
        (bbox.height as f32 * scale_y).round().max(1.0) as u32,
    )
}

fn distance_to_bbox(x: f32, y: f32, bbox: &BoundingBox) -> f32 {
    let left = bbox.x as f32;
    let right = left + bbox.width as f32;
    let top = bbox.y as f32;
    let bottom = top + bbox.height as f32;
    let dx = if x < left {
        left - x
    } else if x > right {
        x - right
    } else {
        0.0
    };
    let dy = if y < top {
        top - y
    } else if y > bottom {
        y - bottom
    } else {
        0.0
    };
    (dx * dx + dy * dy).sqrt()
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn preview_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 48 {
        normalized
    } else {
        let truncated = normalized.chars().take(45).collect::<String>();
        format!("{truncated}...")
    }
}
