use super::audio_capture::{run_audio_loop, AudioCaptureSource};
use super::audio_sources::{
    choose_audio_source, choose_video_source, default_audio_input_name, list_avfoundation_sources,
};
use super::desktop_audio_runtime::stop_live_desktop_capture;
use super::media_io::mediapipe_runtime_available;
use super::types::{
    AvFoundationSources, MultimodalCaptureOptions, MultimodalRuntimeState, MultimodalStartReport,
};
use super::visual_capture::run_visual_loop;
use crate::core::consent::{ConsentManager, Feature};
use crate::core::database::Database;
use crate::core::storage::RecordingStorage;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct MultimodalService {
    db: Arc<Database>,
    storage: Arc<RecordingStorage>,
    consent_manager: Arc<ConsentManager>,
    generation: Arc<AtomicU64>,
    runtime: Arc<Mutex<MultimodalRuntimeState>>,
}

impl MultimodalService {
    pub fn new(
        db: Arc<Database>,
        storage: Arc<RecordingStorage>,
        consent_manager: Arc<ConsentManager>,
    ) -> Self {
        Self {
            db,
            storage,
            consent_manager,
            generation: Arc::new(AtomicU64::new(0)),
            runtime: Arc::new(Mutex::new(MultimodalRuntimeState::default())),
        }
    }

    pub async fn start_capture(
        &self,
        session_id: String,
        options: MultimodalCaptureOptions,
    ) -> Result<MultimodalStartReport, String> {
        self.stop_capture().await;

        let mut report = MultimodalStartReport::default();
        let sources = list_avfoundation_sources().await?;
        let default_audio_source_name = if options.enable_audio {
            default_audio_input_name().await
        } else {
            None
        };
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        if options.enable_visual {
            start_visual_channel(
                self,
                &session_id,
                &sources,
                generation,
                options.display_id,
                &mut report,
            )
            .await?;
        }

        if options.enable_audio {
            start_audio_channel(
                self,
                &session_id,
                &sources,
                generation,
                options.audio_source_id.as_deref(),
                default_audio_source_name.as_deref(),
                options.enable_microphone_audio,
                options.enable_desktop_audio,
                options.desktop_audio_gain_db,
                super::types::AudioAnalysisOptions {
                    transcription_enabled: options.audio_transcription_enabled,
                    speech_emotion_enabled: options.audio_speech_emotion_enabled,
                    sound_events_enabled: options.audio_sound_events_enabled,
                },
                &mut report,
            )
            .await?;
        }

        Ok(report)
    }

    pub async fn stop_capture(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        stop_live_desktop_capture();
        let mut runtime = self.runtime.lock().await;
        runtime.generation = self.generation.load(Ordering::SeqCst);
        if let Some(handle) = runtime.visual_handle.take() {
            handle.abort();
        }
        for handle in runtime.audio_handles.drain(..) {
            handle.abort();
        }
    }
}

async fn start_visual_channel(
    service: &MultimodalService,
    session_id: &str,
    sources: &AvFoundationSources,
    generation: u64,
    display_id: Option<u32>,
    report: &mut MultimodalStartReport,
) -> Result<(), String> {
    if !service
        .consent_manager
        .is_consent_granted(Feature::CameraRecording)
        .await
        .map_err(|e| format!("Failed to inspect camera consent: {e}"))?
    {
        report
            .warnings
            .push("Camera channel is enabled, but camera consent is not granted.".to_string());
        return Ok(());
    }

    let Some(source) = choose_video_source(&sources.video) else {
        report
            .warnings
            .push("No local camera source is available for the vision channel.".to_string());
        return Ok(());
    };

    report.visual_source_name = Some(source.name.clone());
    report.visual_started = true;

    if let Err(error) = mediapipe_runtime_available().await {
        report.warnings.push(format!(
            "MediaPipe runtime is unavailable for the vision channel: {error}"
        ));
        report.visual_started = false;
        return Ok(());
    }

    report.warnings.push(
        "Object detection adapter is unavailable, so visual scenes will omit object labels for now."
            .to_string(),
    );

    let handle = tokio::spawn(run_visual_loop(
        service.db.clone(),
        service.storage.clone(),
        service.generation.clone(),
        generation,
        session_id.to_string(),
        format!("camera:{}", source.index),
        display_id,
        source.name,
        source.index,
    ));
    service.runtime.lock().await.visual_handle = Some(handle);
    Ok(())
}

async fn start_audio_channel(
    service: &MultimodalService,
    session_id: &str,
    sources: &AvFoundationSources,
    generation: u64,
    preferred_source_id: Option<&str>,
    default_source_name: Option<&str>,
    enable_microphone_audio: bool,
    enable_desktop_audio: bool,
    desktop_audio_gain_db: f32,
    analysis_options: super::types::AudioAnalysisOptions,
    report: &mut MultimodalStartReport,
) -> Result<(), String> {
    if !enable_microphone_audio && !enable_desktop_audio {
        report.warnings.push(
            "Audio channel is enabled, but both microphone and desktop audio sources are off."
                .to_string(),
        );
        return Ok(());
    }

    let mut source_names = Vec::new();
    let mut handles = Vec::new();

    if enable_microphone_audio {
        if !service
            .consent_manager
            .is_consent_granted(Feature::MicrophoneRecording)
            .await
            .map_err(|e| format!("Failed to inspect microphone consent: {e}"))?
        {
            report
                .warnings
                .push("Microphone audio is on, but microphone consent is not granted.".to_string());
        } else if !command_available("ffmpeg").await {
            report.warnings.push(
                "Microphone audio could not start because ffmpeg is unavailable.".to_string(),
            );
        } else if let Some(source) =
            choose_audio_source(&sources.audio, preferred_source_id, default_source_name)
        {
            source_names.push(source.name.clone());
            handles.push(tokio::spawn(run_audio_loop(
                service.db.clone(),
                service.storage.clone(),
                service.generation.clone(),
                generation,
                session_id.to_string(),
                format!("microphone:{}", source.index),
                AudioCaptureSource::Microphone {
                    audio_index: source.index,
                },
                analysis_options,
            )));
        } else {
            report
                .warnings
                .push("No local microphone source is available for the audio channel.".to_string());
        }
    }

    if enable_desktop_audio {
        if !service
            .consent_manager
            .is_consent_granted(Feature::ScreenRecording)
            .await
            .map_err(|e| format!("Failed to inspect screen-recording consent: {e}"))?
        {
            report.warnings.push(
                "Desktop audio is on, but Screen Recording permission is not granted.".to_string(),
            );
        } else {
            source_names.push("Desktop audio".to_string());
            handles.push(tokio::spawn(run_audio_loop(
                service.db.clone(),
                service.storage.clone(),
                service.generation.clone(),
                generation,
                session_id.to_string(),
                "desktop_output:system".to_string(),
                AudioCaptureSource::DesktopOutput {
                    gain_db: desktop_audio_gain_db,
                },
                analysis_options,
            )));
        }
    }

    if !handles.is_empty() {
        report.audio_started = true;
        report.audio_source_name = Some(source_names.join(" + "));
        service.runtime.lock().await.audio_handles.extend(handles);
    }
    Ok(())
}

pub(crate) async fn command_available(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}
