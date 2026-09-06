//! Native FluidAudio transcription behind the provider contract.
//!
//! macOS only. First `ensure_initialized` downloads the Parakeet model
//! (~500 MB) and compiles it for the Neural Engine (20-30 s once).

use super::speech_provider::{SpeechProvider, SpeechProviderKind, SpeechTranscript};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Mutex;

pub struct FluidAudioProvider {
    audio: Mutex<fluidaudio_rs::FluidAudio>,
    initialized: Mutex<bool>,
    pub model: String,
}

impl FluidAudioProvider {
    pub fn new() -> Result<Self, String> {
        let audio = fluidaudio_rs::FluidAudio::new()
            .map_err(|error| format!("Failed to create FluidAudio engine: {error}"))?;
        Ok(Self {
            audio: Mutex::new(audio),
            initialized: Mutex::new(false),
            model: "parakeet-tdt (FluidAudio)".to_string(),
        })
    }

    /// Idempotent: downloads + compiles the model on first call only.
    pub fn ensure_initialized(&self) -> Result<(), String> {
        if *self
            .initialized
            .lock()
            .map_err(|_| "FluidAudio lock poisoned.".to_string())?
        {
            return Ok(());
        }
        let guard = self
            .audio
            .lock()
            .map_err(|_| "FluidAudio lock poisoned.".to_string())?;
        guard
            .init_asr()
            .map_err(|error| format!("FluidAudio model setup failed: {error}"))?;
        drop(guard);
        *self
            .initialized
            .lock()
            .map_err(|_| "FluidAudio lock poisoned.".to_string())? = true;
        Ok(())
    }
}

#[async_trait]
impl SpeechProvider for FluidAudioProvider {
    fn kind(&self) -> SpeechProviderKind {
        SpeechProviderKind::NativeHelper
    }

    fn provider_name(&self) -> String {
        "fluidaudio".to_string()
    }

    async fn transcribe_file(&self, audio_path: &Path) -> Result<SpeechTranscript, String> {
        self.ensure_initialized()?;
        let path = audio_path.to_path_buf();
        let model = self.model.clone();
        let (text, confidence) = tokio::task::spawn_blocking(move || {
            // NOTE: a fresh engine per call keeps this Send-safe; the shared
            // engine serves the streaming path once sessions are wired.
            let engine = fluidaudio_rs::FluidAudio::new()
                .map_err(|error| format!("Failed to create FluidAudio engine: {error}"))?;
            engine
                .init_asr()
                .map_err(|error| format!("FluidAudio model setup failed: {error}"))?;
            let result = engine
                .transcribe_file(&path)
                .map_err(|error| format!("FluidAudio transcription failed: {error}"))?;
            Ok::<_, String>((result.text, result.confidence))
        })
        .await
        .map_err(|error| format!("FluidAudio task failed: {error}"))??;
        Ok(SpeechTranscript {
            text,
            language: None,
            confidence: Some(confidence),
            provider: self.provider_name(),
            model,
            is_final: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_identity_without_model_download() {
        // Construction must not touch the network; download happens
        // only in `ensure_initialized`.
        let provider = FluidAudioProvider::new()
            .expect("FluidAudio engine should construct without downloading");
        assert_eq!(provider.kind(), SpeechProviderKind::NativeHelper);
        assert_eq!(provider.provider_name(), "fluidaudio");
    }

    /// One-shot: fetches the default (v3) model + compiles it for the
    /// Neural Engine. Slow (minutes) and network-dependent, so ignored
    /// by default: `cargo test fluidaudio_download -- --ignored`.
    #[test]
    #[ignore]
    fn fluidaudio_download() {
        let provider = FluidAudioProvider::new().expect("engine should construct");
        provider
            .ensure_initialized()
            .expect("model download + ANE compile should succeed");
    }
}
