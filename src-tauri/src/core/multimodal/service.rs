use super::audio_capture::run_audio_loop;
use super::media_io::mediapipe_runtime_available;
use super::types::{
    AvFoundationSource, AvFoundationSources, MultimodalCaptureOptions, MultimodalRuntimeState,
    MultimodalStartReport,
};
use super::visual_capture::run_visual_loop;
use crate::core::consent::{ConsentManager, Feature};
use crate::core::database::Database;
use crate::core::storage::RecordingStorage;
use regex::Regex;
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
            start_audio_channel(self, &session_id, &sources, generation, &mut report).await?;
        }

        Ok(report)
    }

    pub async fn stop_capture(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let mut runtime = self.runtime.lock().await;
        runtime.generation = self.generation.load(Ordering::SeqCst);
        if let Some(handle) = runtime.visual_handle.take() {
            handle.abort();
        }
        if let Some(handle) = runtime.audio_handle.take() {
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
    report: &mut MultimodalStartReport,
) -> Result<(), String> {
    if !service
        .consent_manager
        .is_consent_granted(Feature::MicrophoneRecording)
        .await
        .map_err(|e| format!("Failed to inspect microphone consent: {e}"))?
    {
        report
            .warnings
            .push("Audio channel is enabled, but microphone consent is not granted.".to_string());
        return Ok(());
    }

    let Some(source) = choose_audio_source(&sources.audio) else {
        report
            .warnings
            .push("No local microphone source is available for the audio channel.".to_string());
        return Ok(());
    };

    report.audio_source_name = Some(source.name.clone());
    report.audio_started = true;

    if !command_available("ffmpeg").await {
        report
            .warnings
            .push("Audio channel could not start because ffmpeg is unavailable.".to_string());
        report.audio_started = false;
        return Ok(());
    }
    if !command_available("whisper").await {
        report.warnings.push(
            "Whisper is unavailable, so speech spans will record without ASR transcripts."
                .to_string(),
        );
    }

    let handle = tokio::spawn(run_audio_loop(
        service.db.clone(),
        service.storage.clone(),
        service.generation.clone(),
        generation,
        session_id.to_string(),
        format!("microphone:{}", source.index),
        source.index,
    ));
    service.runtime.lock().await.audio_handle = Some(handle);
    Ok(())
}

pub(crate) async fn list_avfoundation_sources() -> Result<AvFoundationSources, String> {
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-f",
            "avfoundation",
            "-list_devices",
            "true",
            "-i",
            "",
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to inspect AVFoundation devices: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let line_re = Regex::new(r"\[(\d+)\]\s+(.+)$").map_err(|e| e.to_string())?;

    let mut current = "";
    let mut sources = AvFoundationSources::default();
    for line in text.lines() {
        if line.contains("AVFoundation video devices") {
            current = "video";
            continue;
        }
        if line.contains("AVFoundation audio devices") {
            current = "audio";
            continue;
        }
        if let Some(captures) = line_re.captures(line) {
            let index = captures
                .get(1)
                .and_then(|value| value.as_str().parse::<i32>().ok())
                .unwrap_or(-1);
            let name = captures
                .get(2)
                .map(|value| value.as_str().trim().to_string())
                .unwrap_or_default();
            let source = AvFoundationSource { index, name };
            match current {
                "video" => sources.video.push(source),
                "audio" => sources.audio.push(source),
                _ => {}
            }
        }
    }
    Ok(sources)
}

pub(crate) fn choose_video_source(sources: &[AvFoundationSource]) -> Option<AvFoundationSource> {
    sources
        .iter()
        .find(|source| source.name.contains("FaceTime"))
        .or_else(|| {
            sources.iter().find(|source| {
                let lower = source.name.to_lowercase();
                !lower.contains("capture screen") && !lower.contains("desk view")
            })
        })
        .or_else(|| sources.first())
        .cloned()
}

pub(crate) fn choose_audio_source(sources: &[AvFoundationSource]) -> Option<AvFoundationSource> {
    sources
        .iter()
        .find(|source| source.name.contains("MacBook Air Microphone"))
        .or_else(|| {
            sources
                .iter()
                .find(|source| source.name.contains("Microphone"))
        })
        .or_else(|| sources.first())
        .cloned()
}

pub(crate) async fn command_available(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}
