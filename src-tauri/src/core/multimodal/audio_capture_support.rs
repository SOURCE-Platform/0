use super::audio_runtime::{
    run_parakeet_transcription, run_sound_event_inference, run_speech_emotion_inference,
};
use super::constants::{
    AUDIO_VAD_WINDOW_MS, SOUND_EVENT_MIN_CONFIDENCE, SOUND_EVENT_MODEL_NAME,
    SOUND_EVENT_MODEL_VERSION, SPEECH_EMOTION_MIN_CONFIDENCE, SPEECH_EMOTION_MODEL_NAME,
    SPEECH_EMOTION_MODEL_VERSION, VAD_RMS_INTERMITTENT_THRESHOLD, VAD_RMS_SPEECH_THRESHOLD,
};
use crate::core::database::Database;
use hound::WavReader;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

pub(super) fn classify_audio_trigger_reason(
    previous_speech_detected: bool,
    speech_detected: bool,
    vad_score: f32,
) -> &'static str {
    if speech_detected && !previous_speech_detected {
        "speech_started"
    } else if !speech_detected && previous_speech_detected {
        "speech_stopped"
    } else if vad_score > 0.0 {
        "static_fallback"
    } else {
        "periodic_sample"
    }
}

pub(crate) struct AudioAnalysis {
    pub vad_score: f32,
    pub speech_detected: bool,
    pub waveform_levels: Vec<f32>,
}

pub(crate) fn analyze_audio_chunk(path: &Path) -> Result<AudioAnalysis, String> {
    let mut reader = WavReader::open(path).map_err(|e| format!("Failed to open WAV chunk: {e}"))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate as usize;
    let window_samples = (sample_rate * AUDIO_VAD_WINDOW_MS) / 1000;

    let waveform_window_samples = (sample_rate / 20).max(1);
    let mut waveform_window = Vec::with_capacity(waveform_window_samples);
    let mut waveform_levels = Vec::with_capacity(24);
    let mut window = Vec::with_capacity(window_samples.max(1));
    let mut max_rms = 0.0f32;
    let mut speech_windows = 0usize;
    let mut total_windows = 0usize;

    for sample in reader.samples::<i16>() {
        let sample = sample.map_err(|e| format!("Failed to read WAV sample: {e}"))?;
        let normalized = sample as f32 / i16::MAX as f32;
        window.push(normalized);
        waveform_window.push(normalized);
        if waveform_window.len() >= waveform_window_samples {
            waveform_levels.push((rms_window(&waveform_window) * 12.0).clamp(0.03, 1.0));
            waveform_window.clear();
        }
        if window.len() >= window_samples.max(1) {
            let rms = rms_window(&window);
            max_rms = max_rms.max(rms);
            if rms >= VAD_RMS_SPEECH_THRESHOLD {
                speech_windows += 1;
            }
            total_windows += 1;
            window.clear();
        }
    }
    if !window.is_empty() {
        let rms = rms_window(&window);
        max_rms = max_rms.max(rms);
        if rms >= VAD_RMS_SPEECH_THRESHOLD {
            speech_windows += 1;
        }
        total_windows += 1;
    }

    let speech_detected = speech_windows >= 2
        || (total_windows > 0 && max_rms >= VAD_RMS_INTERMITTENT_THRESHOLD && speech_windows > 0);
    if !waveform_window.is_empty() {
        waveform_levels.push((rms_window(&waveform_window) * 12.0).clamp(0.03, 1.0));
    }
    Ok(AudioAnalysis {
        vad_score: max_rms,
        speech_detected,
        waveform_levels,
    })
}

pub(super) async fn persist_speech_emotion(
    db: &Arc<Database>,
    temp_path: &Path,
    session_id: &str,
    source_id: &str,
    audio_chunk_id: &str,
    asr_segment_id: Option<String>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<(), String> {
    let inference = run_speech_emotion_inference(temp_path).await?;
    if !inference.available || inference.confidence < SPEECH_EMOTION_MIN_CONFIDENCE {
        return Ok(());
    }
    let emotion_label = inference.label.unwrap_or_else(|| "unknown".to_string());
    let canonical_label = inference
        .canonical_label
        .unwrap_or_else(|| "uncertain".to_string());
    let model_name = if inference.model_name.is_empty() {
        SPEECH_EMOTION_MODEL_NAME.to_string()
    } else {
        inference.model_name
    };
    let model_version = if inference.model_version.is_empty() {
        SPEECH_EMOTION_MODEL_VERSION.to_string()
    } else {
        inference.model_version
    };

    sqlx::query(
        "INSERT INTO speech_emotion_segments (
            speech_emotion_segment_id, session_id, source_id, audio_chunk_id, asr_segment_id,
            start_timestamp, end_timestamp, trigger_reason, emotion_label, canonical_label,
            confidence, model_name, model_version, raw_json, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(session_id)
    .bind(source_id)
    .bind(audio_chunk_id)
    .bind(asr_segment_id)
    .bind(start_timestamp)
    .bind(end_timestamp)
    .bind("speech_emotion_promoted")
    .bind(emotion_label)
    .bind(canonical_label)
    .bind(inference.confidence as f64)
    .bind(model_name)
    .bind(model_version)
    .bind(
        json!({
            "top_candidates": inference.top_candidates,
            "raw": inference.raw_json,
            "notes": inference.notes,
        })
        .to_string(),
    )
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(db.pool())
    .await
    .map_err(|e| format!("Failed to persist speech emotion segment: {e}"))?;

    Ok(())
}

pub(super) async fn persist_sound_events(
    db: &Arc<Database>,
    temp_path: &Path,
    session_id: &str,
    source_id: &str,
    audio_chunk_id: &str,
    start_timestamp: i64,
    end_timestamp: i64,
    speech_detected: bool,
) -> Result<(), String> {
    let inference = run_sound_event_inference(temp_path).await?;
    if !inference.available {
        return Ok(());
    }

    let model_name = if inference.model_name.is_empty() {
        SOUND_EVENT_MODEL_NAME.to_string()
    } else {
        inference.model_name.clone()
    };
    let model_version = if inference.model_version.is_empty() {
        SOUND_EVENT_MODEL_VERSION.to_string()
    } else {
        inference.model_version.clone()
    };

    for event in inference.events {
        if event.confidence < SOUND_EVENT_MIN_CONFIDENCE {
            continue;
        }
        if speech_detected && event.canonical_label == "speech" {
            continue;
        }
        sqlx::query(
            "INSERT INTO sound_event_detections (
                sound_event_detection_id, session_id, source_id, audio_chunk_id,
                start_timestamp, end_timestamp, trigger_reason, event_label, canonical_label,
                confidence, model_name, model_version, raw_json, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(source_id)
        .bind(audio_chunk_id)
        .bind(start_timestamp)
        .bind(end_timestamp)
        .bind("sound_event_detected")
        .bind(event.label)
        .bind(event.canonical_label)
        .bind(event.confidence as f64)
        .bind(model_name.clone())
        .bind(model_version.clone())
        .bind(json!({"raw": inference.raw_json, "notes": inference.notes}).to_string())
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(db.pool())
        .await
        .map_err(|e| format!("Failed to persist sound event detection: {e}"))?;
    }

    Ok(())
}

pub(super) async fn transcribe_audio_chunk(
    path: &Path,
) -> Option<super::audio_runtime::ParakeetTranscription> {
    match run_parakeet_transcription(path).await {
        Ok(result) if !result.text.trim().is_empty() => Some(result),
        Ok(_) => None,
        Err(error) => {
            eprintln!("ambient Parakeet transcription failed: {error}");
            None
        }
    }
}

fn rms_window(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}
