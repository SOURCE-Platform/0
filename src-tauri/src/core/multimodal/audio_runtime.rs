use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

const AUDIO_IMPORT_CHECK: &str = "import numpy, onnxruntime; print('ok')";
const AUDIO_PACKAGES: &[&str] = &["numpy==1.26.4", "onnxruntime==1.18.1"];

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpeechEmotionInference {
    pub available: bool,
    pub label: Option<String>,
    pub canonical_label: Option<String>,
    pub confidence: f32,
    pub model_name: String,
    pub model_version: String,
    pub top_candidates: Vec<RankedInferenceLabel>,
    pub raw_json: serde_json::Value,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RankedInferenceLabel {
    pub index: i64,
    pub label: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SoundEventInference {
    pub available: bool,
    pub model_name: String,
    pub model_version: String,
    pub events: Vec<SoundEventCandidate>,
    pub raw_json: serde_json::Value,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SoundEventCandidate {
    pub label: String,
    pub canonical_label: String,
    pub confidence: f32,
}

pub(super) async fn run_speech_emotion_inference(
    audio_path: &Path,
) -> Result<SpeechEmotionInference, String> {
    run_audio_helper::<SpeechEmotionInference>("speech_emotion", audio_path).await
}

pub(super) async fn run_sound_event_inference(
    audio_path: &Path,
) -> Result<SoundEventInference, String> {
    run_audio_helper::<SoundEventInference>("sound_events", audio_path).await
}

async fn run_audio_helper<T>(mode: &str, audio_path: &Path) -> Result<T, String>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let python_path = ensure_audio_python().await?;
    let script_path = ensure_audio_runtime_files().await?;
    let output = Command::new(&python_path)
        .arg(script_path)
        .arg(mode)
        .arg(audio_path)
        .output()
        .await
        .map_err(|e| format!("Failed to run audio understanding helper: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    serde_json::from_slice::<T>(&output.stdout)
        .map_err(|e| format!("Failed to parse audio understanding output: {e}"))
}

async fn ensure_audio_runtime_files() -> Result<PathBuf, String> {
    let helpers_dir = helper_dir()?;
    fs::create_dir_all(&helpers_dir).map_err(|e| format!("Failed to create helpers dir: {e}"))?;
    let script_path = helpers_dir.join("audio_understanding_runtime.py");
    write_helper_if_needed(
        &script_path,
        include_str!("../helpers/audio_understanding_runtime.py"),
    )?;
    Ok(script_path)
}

async fn ensure_audio_python() -> Result<PathBuf, String> {
    let helpers_dir = helper_dir()?;
    fs::create_dir_all(&helpers_dir).map_err(|e| format!("Failed to create helpers dir: {e}"))?;

    let venv_dir = helpers_dir.join(".venv");
    let venv_python = venv_dir.join("bin").join("python3");
    if venv_python.exists() && python_supports_audio(&venv_python).await? {
        return Ok(venv_python);
    }

    let base_python = discover_python3().await?;
    if !venv_python.exists() {
        let status = Command::new(&base_python)
            .args(["-m", "venv"])
            .arg(&venv_dir)
            .status()
            .await
            .map_err(|e| format!("Failed to create SOURCE audio runtime: {e}"))?;
        if !status.success() {
            return Err("Could not create a local SOURCE audio runtime.".to_string());
        }
    }

    if !python_supports_audio(&venv_python).await? {
        let status = Command::new(&venv_python)
            .args(["-m", "pip", "install", "--quiet"])
            .args(AUDIO_PACKAGES)
            .status()
            .await
            .map_err(|e| format!("Failed to install SOURCE audio runtime packages: {e}"))?;
        if !status.success() {
            return Err("SOURCE could not install the local audio helper runtime.".to_string());
        }
    }

    if python_supports_audio(&venv_python).await? {
        Ok(venv_python)
    } else {
        Err("SOURCE still cannot import the audio runtime after setup.".to_string())
    }
}

async fn python_supports_audio(python_path: &Path) -> Result<bool, String> {
    let output = Command::new(python_path)
        .args(["-c", AUDIO_IMPORT_CHECK])
        .output()
        .await
        .map_err(|e| format!("Failed to inspect SOURCE audio runtime: {e}"))?;
    Ok(output.status.success())
}

async fn discover_python3() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        PathBuf::from(format!("{home}/.pyenv/shims/python3")),
        PathBuf::from("/opt/homebrew/bin/python3"),
        PathBuf::from("/usr/local/bin/python3"),
        PathBuf::from("/usr/bin/python3"),
        PathBuf::from("python3"),
    ];

    for candidate in candidates {
        let output = Command::new(&candidate).arg("--version").output().await;
        if matches!(output, Ok(ref value) if value.status.success()) {
            return Ok(candidate);
        }
    }

    Err("SOURCE could not find a usable Python 3 runtime for audio setup.".to_string())
}

fn helper_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "Could not resolve home directory for SOURCE helpers.".to_string())?;
    Ok(PathBuf::from(home).join(".observer_data").join("helpers"))
}

fn write_helper_if_needed(path: &Path, contents: &str) -> Result<(), String> {
    let should_write = match fs::read_to_string(path) {
        Ok(existing) => existing != contents,
        Err(_) => true,
    };
    if should_write {
        fs::write(path, contents).map_err(|e| format!("Failed to write audio helper: {e}"))?;
    }
    Ok(())
}
