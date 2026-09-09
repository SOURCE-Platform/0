use crate::app::state::AppState;
use crate::core::multimodal::{
    self, AsrSegmentDto, AudioStateSpanDto, MultimodalActivityEpisodeDto, SoundEventDetectionDto,
    SoundEventSpanDto, SpeechEmotionSegmentDto, VisualAudioSummaryDto, VisualSceneSnapshotDto,
    VisualStateSpanDto,
};
use crate::core::multimodal::{
    current_audio_meters, default_audio_input_name, list_avfoundation_sources,
    prepare_audio_meters, sample_audio_meters, AudioMetersDto,
};
use serde::Serialize;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

static AUDIO_METER_PUBLISHER: OnceLock<Mutex<Option<AudioMeterPublisher>>> = OnceLock::new();

struct AudioMeterPublisher {
    stop_tx: Sender<()>,
    _thread: JoinHandle<()>,
}

impl Drop for AudioMeterPublisher {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputSourceDto {
    pub source_id: String,
    pub name: String,
    pub index: i32,
    pub is_system_default: bool,
}

#[tauri::command]
pub async fn get_visual_scene_snapshot(
    visual_scene_id: String,
    state: State<'_, AppState>,
) -> Result<Option<VisualSceneSnapshotDto>, String> {
    multimodal::get_visual_scene_snapshot(&state.db, &visual_scene_id)
        .await
        .map_err(|e| format!("Failed to get visual scene snapshot: {}", e))
}

#[tauri::command]
pub async fn get_visual_scene_snapshots(
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<VisualSceneSnapshotDto>, String> {
    multimodal::get_visual_scene_snapshots(&state.db, start_timestamp, end_timestamp, source_filter)
        .await
        .map_err(|e| format!("Failed to get visual scene snapshots: {}", e))
}

#[tauri::command]
pub async fn get_visual_state_spans(
    start_timestamp: i64,
    end_timestamp: i64,
    state_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<VisualStateSpanDto>, String> {
    multimodal::get_visual_state_spans(&state.db, start_timestamp, end_timestamp, state_type)
        .await
        .map_err(|e| format!("Failed to get visual state spans: {}", e))
}

#[tauri::command]
pub async fn get_audio_state_spans(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<Vec<AudioStateSpanDto>, String> {
    multimodal::get_audio_state_spans(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get audio state spans: {}", e))
}

#[tauri::command]
pub async fn get_asr_segments(
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<AsrSegmentDto>, String> {
    multimodal::get_asr_segments(&state.db, start_timestamp, end_timestamp, source_filter)
        .await
        .map_err(|e| format!("Failed to get ASR segments: {}", e))
}

#[tauri::command]
pub async fn get_asr_segment(
    asr_segment_id: String,
    state: State<'_, AppState>,
) -> Result<Option<AsrSegmentDto>, String> {
    multimodal::get_asr_segment(&state.db, &asr_segment_id)
        .await
        .map_err(|e| format!("Failed to get ASR segment: {e}"))
}

#[tauri::command]
pub async fn get_speech_emotion_segments(
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<SpeechEmotionSegmentDto>, String> {
    multimodal::get_speech_emotion_segments(
        &state.db,
        start_timestamp,
        end_timestamp,
        source_filter,
    )
    .await
    .map_err(|e| format!("Failed to get speech emotion segments: {}", e))
}

#[tauri::command]
pub async fn get_sound_event_detections(
    start_timestamp: i64,
    end_timestamp: i64,
    source_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<SoundEventDetectionDto>, String> {
    multimodal::get_sound_event_detections(&state.db, start_timestamp, end_timestamp, source_filter)
        .await
        .map_err(|e| format!("Failed to get sound event detections: {}", e))
}

#[tauri::command]
pub async fn get_sound_event_spans(
    start_timestamp: i64,
    end_timestamp: i64,
    label_filter: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<SoundEventSpanDto>, String> {
    multimodal::get_sound_event_spans(&state.db, start_timestamp, end_timestamp, label_filter)
        .await
        .map_err(|e| format!("Failed to get sound event spans: {}", e))
}

#[tauri::command]
pub async fn get_visual_audio_summary(
    start_timestamp: i64,
    end_timestamp: i64,
    state: State<'_, AppState>,
) -> Result<VisualAudioSummaryDto, String> {
    multimodal::get_visual_audio_summary(&state.db, start_timestamp, end_timestamp)
        .await
        .map_err(|e| format!("Failed to get visual/audio summary: {}", e))
}

#[tauri::command]
pub async fn get_multimodal_activity_episode(
    timestamp: i64,
    state: State<'_, AppState>,
) -> Result<MultimodalActivityEpisodeDto, String> {
    multimodal::get_multimodal_activity_episode(&state.db, timestamp)
        .await
        .map_err(|e| format!("Failed to get multimodal activity episode: {}", e))
}

#[tauri::command]
pub async fn list_audio_input_sources() -> Result<Vec<AudioInputSourceDto>, String> {
    let sources = list_avfoundation_sources().await?;
    let default_source_name = default_audio_input_name().await;
    Ok(sources
        .audio
        .into_iter()
        .map(|source| AudioInputSourceDto {
            source_id: format!("microphone:{}", source.index),
            is_system_default: default_source_name.as_deref() == Some(source.name.as_str()),
            name: source.name,
            index: source.index,
        })
        .collect())
}

#[tauri::command]
pub async fn get_audio_source_meters(
    selected_audio_input_id: Option<String>,
    desktop_audio_enabled: Option<bool>,
    desktop_audio_gain_db: Option<f32>,
) -> Result<AudioMetersDto, String> {
    sample_audio_meters(
        selected_audio_input_id.as_deref(),
        desktop_audio_enabled.unwrap_or(false),
        desktop_audio_gain_db.unwrap_or(12.0),
    )
    .await
}

/// Starts one 30 Hz backend feed. The frontend only renders emitted frames.
#[tauri::command]
pub async fn start_audio_meter_stream(
    selected_audio_input_id: Option<String>,
    desktop_audio_enabled: bool,
    desktop_audio_gain_db: f32,
    app: AppHandle,
) -> Result<AudioMetersDto, String> {
    let initial = prepare_audio_meters(
        selected_audio_input_id.as_deref(),
        desktop_audio_enabled,
        desktop_audio_gain_db,
    )
    .await?;
    let publisher = AUDIO_METER_PUBLISHER.get_or_init(|| Mutex::new(None));
    let mut active = publisher
        .lock()
        .map_err(|_| "Audio meter publisher lock is unavailable.".to_string())?;

    *active = Some(spawn_audio_meter_publisher(
        app,
        desktop_audio_enabled,
        desktop_audio_gain_db,
    ));
    Ok(initial)
}

#[tauri::command]
pub fn stop_audio_meter_stream() -> Result<(), String> {
    let Some(publisher) = AUDIO_METER_PUBLISHER.get() else {
        return Ok(());
    };
    let mut active = publisher
        .lock()
        .map_err(|_| "Audio meter publisher lock is unavailable.".to_string())?;
    *active = None;
    Ok(())
}

fn spawn_audio_meter_publisher(
    app: AppHandle,
    desktop_audio_enabled: bool,
    desktop_audio_gain_db: f32,
) -> AudioMeterPublisher {
    let (stop_tx, stop_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        // Keep transport steady at 30 Hz. The audio callback itself updates the
        // latest RMS value more often, using the device's native buffer cadence.
        while stop_rx.recv_timeout(Duration::from_millis(33)).is_err() {
            let _ = app.emit(
                "audio-meter-frame",
                current_audio_meters(desktop_audio_enabled, desktop_audio_gain_db),
            );
        }
    });
    AudioMeterPublisher {
        stop_tx,
        _thread: handle,
    }
}
