use super::types::{FaceFeatureSampleDto, GazeCalibrationDto, GazeCalibrationPointDto};
use serde_json::json;
use serde_json::Value;

#[derive(Debug, Clone)]
pub(crate) struct PredictedGazePoint {
    pub screen_x: f32,
    pub screen_y: f32,
    pub confidence: f32,
    pub accuracy_radius_px: f32,
    pub head_pose: Option<Value>,
    pub face_bbox: Option<Value>,
}

pub(crate) fn predict_gaze_point(
    calibration: &GazeCalibrationDto,
    features: &FaceFeatureSampleDto,
) -> Option<PredictedGazePoint> {
    if calibration.calibration_points.is_empty() || features.confidence <= 0.0 {
        return None;
    }

    let sample_vector = feature_vector(features);
    let mut scored = calibration
        .calibration_points
        .iter()
        .filter_map(|point| {
            let vector = feature_vector(&point.features);
            let distance = weighted_distance(&sample_vector, &vector);
            distance.is_finite().then_some((point, distance))
        })
        .collect::<Vec<_>>();

    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|a, b| a.1.total_cmp(&b.1));
    let neighbors = scored.into_iter().take(4).collect::<Vec<_>>();

    let mut weighted_x = 0.0_f32;
    let mut weighted_y = 0.0_f32;
    let mut weight_sum = 0.0_f32;
    let mut spread_weighted = 0.0_f32;
    for (point, distance) in &neighbors {
        let weight = 1.0 / (distance + 0.001);
        weighted_x += point.target_x * weight;
        weighted_y += point.target_y * weight;
        weight_sum += weight;
        spread_weighted += *distance * weight;
    }

    let screen_x = (weighted_x / weight_sum).clamp(0.0, calibration.screen_width as f32);
    let screen_y = (weighted_y / weight_sum).clamp(0.0, calibration.screen_height as f32);
    let average_distance = (spread_weighted / weight_sum).max(0.0);
    let validation_error = calibration.validation_error_px.unwrap_or(180.0);
    let mut accuracy_radius_px = (validation_error * 0.7 + average_distance * 900.0).max(72.0);
    let mut confidence =
        (features.confidence * (1.0 / (1.0 + average_distance * 10.0))).clamp(0.05, 0.98);

    if let Some(head_pose_range) = calibration.head_pose_range.as_ref() {
        let drift = head_pose_drift_score(head_pose_range, features);
        confidence *= (1.0 - drift * 0.45).clamp(0.25, 1.0);
        accuracy_radius_px *= 1.0 + drift;
    }

    Some(PredictedGazePoint {
        screen_x,
        screen_y,
        confidence,
        accuracy_radius_px,
        head_pose: Some(json!({
            "yaw": features.yaw,
            "pitch": features.pitch,
            "roll": features.roll,
            "face_center_x": features.face_center_x,
            "face_center_y": features.face_center_y,
            "face_width": features.face_width,
            "face_height": features.face_height,
        })),
        face_bbox: features.face_bbox.clone(),
    })
}

pub(crate) fn validate_calibration_points(calibration: &GazeCalibrationDto) -> f32 {
    if calibration.calibration_points.len() < 3 {
        return 480.0;
    }

    let mut errors = Vec::new();
    for (index, point) in calibration.calibration_points.iter().enumerate() {
        let others = calibration
            .calibration_points
            .iter()
            .enumerate()
            .filter_map(|(other_index, other)| (other_index != index).then_some(other.clone()))
            .collect::<Vec<_>>();
        let temp = GazeCalibrationDto {
            calibration_points: others,
            ..calibration.clone()
        };
        if let Some(predicted) = predict_gaze_point(&temp, &point.features) {
            let dx = predicted.screen_x - point.target_x;
            let dy = predicted.screen_y - point.target_y;
            errors.push((dx * dx + dy * dy).sqrt());
        }
    }

    if errors.is_empty() {
        480.0
    } else {
        errors.iter().sum::<f32>() / errors.len() as f32
    }
}

pub(crate) fn quality_bucket_for_error(error_px: f32) -> &'static str {
    if error_px <= 120.0 {
        "excellent"
    } else if error_px <= 260.0 {
        "usable"
    } else if error_px <= 520.0 {
        "weak"
    } else {
        "failed"
    }
}

pub(crate) fn build_head_pose_range(points: &[GazeCalibrationPointDto]) -> Option<Value> {
    if points.is_empty() {
        return None;
    }

    let yaw = min_max(points.iter().filter_map(|point| point.features.yaw));
    let pitch = min_max(points.iter().filter_map(|point| point.features.pitch));
    let roll = min_max(points.iter().filter_map(|point| point.features.roll));
    let face_center_x = min_max(points.iter().map(|point| point.features.face_center_x));
    let face_center_y = min_max(points.iter().map(|point| point.features.face_center_y));
    let face_width = min_max(points.iter().map(|point| point.features.face_width));
    let face_height = min_max(points.iter().map(|point| point.features.face_height));

    Some(json!({
        "yaw": yaw,
        "pitch": pitch,
        "roll": roll,
        "face_center_x": face_center_x,
        "face_center_y": face_center_y,
        "face_width": face_width,
        "face_height": face_height,
    }))
}

fn feature_vector(features: &FaceFeatureSampleDto) -> [f32; 10] {
    [
        features.face_center_x,
        features.face_center_y,
        features.face_width,
        features.face_height,
        features.eye_mid_x.unwrap_or(features.face_center_x),
        features.eye_mid_y.unwrap_or(features.face_center_y),
        features.inter_eye_distance.unwrap_or(0.0),
        features
            .gaze_yaw_degrees
            .unwrap_or(features.yaw.unwrap_or(0.0)),
        features
            .gaze_pitch_degrees
            .unwrap_or(features.pitch.unwrap_or(0.0)),
        features.roll.unwrap_or(0.0),
    ]
}

fn weighted_distance(a: &[f32; 10], b: &[f32; 10]) -> f32 {
    const WEIGHTS: [f32; 10] = [1.0, 1.0, 0.8, 0.8, 1.2, 1.2, 1.0, 0.9, 0.6, 0.6];
    a.iter()
        .zip(b.iter())
        .zip(WEIGHTS.iter())
        .map(|((left, right), weight)| {
            let diff = left - right;
            diff * diff * weight
        })
        .sum::<f32>()
        .sqrt()
}

fn head_pose_drift_score(range: &Value, features: &FaceFeatureSampleDto) -> f32 {
    [
        drift_against_range(range.get("yaw"), features.yaw),
        drift_against_range(range.get("pitch"), features.pitch),
        drift_against_range(range.get("roll"), features.roll),
        drift_against_range(range.get("face_center_x"), Some(features.face_center_x)),
        drift_against_range(range.get("face_center_y"), Some(features.face_center_y)),
        drift_against_range(range.get("face_width"), Some(features.face_width)),
        drift_against_range(range.get("face_height"), Some(features.face_height)),
    ]
    .into_iter()
    .sum::<f32>()
        / 7.0
}

fn drift_against_range(range_value: Option<&Value>, current: Option<f32>) -> f32 {
    let Some(current) = current else {
        return 1.0;
    };
    let Some(range_value) = range_value else {
        return 0.0;
    };
    let min = range_value
        .get("min")
        .and_then(Value::as_f64)
        .unwrap_or(current as f64) as f32;
    let max = range_value
        .get("max")
        .and_then(Value::as_f64)
        .unwrap_or(current as f64) as f32;
    let span = (max - min).abs().max(0.01);
    if current >= min && current <= max {
        0.0
    } else if current < min {
        ((min - current) / span).clamp(0.0, 2.0)
    } else {
        ((current - max) / span).clamp(0.0, 2.0)
    }
}

fn min_max(values: impl Iterator<Item = f32>) -> Value {
    let items = values.collect::<Vec<_>>();
    let min = items.iter().copied().reduce(f32::min).unwrap_or(0.0);
    let max = items.iter().copied().reduce(f32::max).unwrap_or(0.0);
    json!({
        "min": min,
        "max": max,
    })
}
