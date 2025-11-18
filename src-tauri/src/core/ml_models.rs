// ML Model loader and manager utilities
// Handles model downloading, caching, and initialization

use std::path::{Path, PathBuf};
use std::fs;

/// Model source configuration
#[derive(Debug, Clone)]
pub enum ModelSource {
    /// Local file path
    LocalFile(PathBuf),
    /// Hugging Face model hub
    HuggingFace { repo: String, filename: String },
    /// Direct URL
    Url(String),
}

/// ML model metadata
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub version: String,
    pub source: ModelSource,
    pub size_bytes: Option<u64>,
    pub checksum: Option<String>,
}

/// Model manager for caching and loading ML models
pub struct ModelManager {
    cache_dir: PathBuf,
}

impl ModelManager {
    /// Create a new model manager with cache directory
    pub fn new(cache_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        fs::create_dir_all(&cache_dir)?;
        Ok(Self { cache_dir })
    }

    /// Get the cache directory path
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Check if a model is cached
    pub fn is_cached(&self, model: &ModelInfo) -> bool {
        let model_path = self.get_model_path(&model.name);
        model_path.exists()
    }

    /// Get the local path for a model
    pub fn get_model_path(&self, model_name: &str) -> PathBuf {
        self.cache_dir.join(model_name)
    }

    /// Download a model if not cached
    pub async fn ensure_model(&self, model: &ModelInfo) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let model_path = self.get_model_path(&model.name);

        if self.is_cached(model) {
            println!("Model {} already cached at {:?}", model.name, model_path);
            return Ok(model_path);
        }

        println!("Downloading model {} from {:?}", model.name, model.source);

        match &model.source {
            ModelSource::LocalFile(path) => {
                // Copy local file to cache
                fs::copy(path, &model_path)?;
            }
            ModelSource::HuggingFace { repo, filename } => {
                // TODO: Download from Hugging Face Hub
                // Use hf-hub crate or direct API calls
                eprintln!("HuggingFace download not yet implemented: {}/{}", repo, filename);
                return Err("HuggingFace download not implemented".into());
            }
            ModelSource::Url(url) => {
                // TODO: Download from URL
                // Use reqwest to download file
                eprintln!("URL download not yet implemented: {}", url);
                return Err("URL download not implemented".into());
            }
        }

        Ok(model_path)
    }

    /// Clear the model cache
    pub fn clear_cache(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir)?;
            fs::create_dir_all(&self.cache_dir)?;
        }
        Ok(())
    }

    /// Get cache size in bytes
    pub fn get_cache_size(&self) -> Result<u64, Box<dyn std::error::Error>> {
        let mut total_size = 0u64;

        if self.cache_dir.exists() {
            for entry in fs::read_dir(&self.cache_dir)? {
                let entry = entry?;
                let metadata = entry.metadata()?;
                if metadata.is_file() {
                    total_size += metadata.len();
                }
            }
        }

        Ok(total_size)
    }
}

// ==============================================================================
// Predefined Model Configurations
// ==============================================================================

/// Whisper model configurations
pub mod whisper {
    use super::*;

    pub fn tiny() -> ModelInfo {
        ModelInfo {
            name: "whisper-tiny".to_string(),
            version: "v3".to_string(),
            source: ModelSource::HuggingFace {
                repo: "openai/whisper-tiny".to_string(),
                filename: "model.bin".to_string(),
            },
            size_bytes: Some(39_000_000), // ~39 MB
            checksum: None,
        }
    }

    pub fn base() -> ModelInfo {
        ModelInfo {
            name: "whisper-base".to_string(),
            version: "v3".to_string(),
            source: ModelSource::HuggingFace {
                repo: "openai/whisper-base".to_string(),
                filename: "model.bin".to_string(),
            },
            size_bytes: Some(74_000_000), // ~74 MB
            checksum: None,
        }
    }

    pub fn small() -> ModelInfo {
        ModelInfo {
            name: "whisper-small".to_string(),
            version: "v3".to_string(),
            source: ModelSource::HuggingFace {
                repo: "openai/whisper-small".to_string(),
                filename: "model.bin".to_string(),
            },
            size_bytes: Some(244_000_000), // ~244 MB
            checksum: None,
        }
    }
}

/// MediaPipe ONNX model configurations
pub mod mediapipe {
    use super::*;

    pub fn pose_landmark() -> ModelInfo {
        ModelInfo {
            name: "mediapipe-pose".to_string(),
            version: "v1".to_string(),
            source: ModelSource::Url(
                "https://storage.googleapis.com/mediapipe-models/pose_landmarker/pose_landmarker_heavy/float16/latest/pose_landmarker_heavy.task".to_string()
            ),
            size_bytes: Some(13_000_000), // ~13 MB
            checksum: None,
        }
    }

    pub fn face_mesh() -> ModelInfo {
        ModelInfo {
            name: "mediapipe-face-mesh".to_string(),
            version: "v1".to_string(),
            source: ModelSource::Url(
                "https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/latest/face_landmarker.task".to_string()
            ),
            size_bytes: Some(11_000_000), // ~11 MB
            checksum: None,
        }
    }

    pub fn hand_landmarker() -> ModelInfo {
        ModelInfo {
            name: "mediapipe-hands".to_string(),
            version: "v1".to_string(),
            source: ModelSource::Url(
                "https://storage.googleapis.com/mediapipe-models/hand_landmarker/hand_landmarker/float16/latest/hand_landmarker.task".to_string()
            ),
            size_bytes: Some(10_000_000), // ~10 MB
            checksum: None,
        }
    }
}

/// pyannote.audio model configurations
pub mod pyannote {
    use super::*;

    pub fn speaker_diarization() -> ModelInfo {
        ModelInfo {
            name: "pyannote-diarization".to_string(),
            version: "3.1".to_string(),
            source: ModelSource::HuggingFace {
                repo: "pyannote/speaker-diarization-3.1".to_string(),
                filename: "pytorch_model.bin".to_string(),
            },
            size_bytes: Some(17_000_000), // ~17 MB
            checksum: None,
        }
    }

    pub fn embedding() -> ModelInfo {
        ModelInfo {
            name: "pyannote-embedding".to_string(),
            version: "3.1".to_string(),
            source: ModelSource::HuggingFace {
                repo: "pyannote/embedding".to_string(),
                filename: "pytorch_model.bin".to_string(),
            },
            size_bytes: Some(17_000_000), // ~17 MB
            checksum: None,
        }
    }
}

/// SpeechBrain emotion detection models
pub mod speechbrain {
    use super::*;

    pub fn emotion_recognition() -> ModelInfo {
        ModelInfo {
            name: "speechbrain-emotion".to_string(),
            version: "1.0".to_string(),
            source: ModelSource::HuggingFace {
                repo: "speechbrain/emotion-recognition-wav2vec2-IEMOCAP".to_string(),
                filename: "pytorch_model.bin".to_string(),
            },
            size_bytes: Some(378_000_000), // ~378 MB
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_manager_creation() {
        let temp_dir = std::env::temp_dir().join("test_models");
        let manager = ModelManager::new(temp_dir.clone()).unwrap();
        assert_eq!(manager.cache_dir(), temp_dir.as_path());
    }

    #[test]
    fn test_whisper_models() {
        let tiny = whisper::tiny();
        assert_eq!(tiny.name, "whisper-tiny");
        assert!(tiny.size_bytes.unwrap() > 0);

        let base = whisper::base();
        assert_eq!(base.name, "whisper-base");
        assert!(base.size_bytes.unwrap() > tiny.size_bytes.unwrap());
    }

    #[test]
    fn test_mediapipe_models() {
        let pose = mediapipe::pose_landmark();
        assert_eq!(pose.name, "mediapipe-pose");

        let face = mediapipe::face_mesh();
        assert_eq!(face.name, "mediapipe-face-mesh");

        let hands = mediapipe::hand_landmarker();
        assert_eq!(hands.name, "mediapipe-hands");
    }
}
