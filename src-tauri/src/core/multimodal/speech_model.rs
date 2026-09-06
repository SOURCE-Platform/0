use std::path::PathBuf;

/// One installed model copy shared by background transcription and
/// foreground dictation. Only one variant is resident at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpeechModelId {
    ParakeetV2,
    ParakeetV3,
}

impl SpeechModelId {
    pub fn display_name(&self) -> &'static str {
        match self {
            SpeechModelId::ParakeetV2 => "Parakeet TDT v2 (English)",
            SpeechModelId::ParakeetV3 => "Parakeet TDT v3 (Multilingual)",
        }
    }

    pub fn approx_bytes(&self) -> u64 {
        match self {
            SpeechModelId::ParakeetV2 => 464_421_712,
            SpeechModelId::ParakeetV3 => 483_288_717,
        }
    }

    pub fn languages(&self) -> &'static str {
        match self {
            SpeechModelId::ParakeetV2 => "en",
            SpeechModelId::ParakeetV3 => "bg,hr,cs,da,nl,en,et,fi,fr,de,el,hu,it,lv,lt,mt,pl,pt,ro,ru,sk,sl,es,sv,uk",
        }
    }

    pub fn supports_polish(&self) -> bool {
        matches!(self, SpeechModelId::ParakeetV3)
    }

    /// Directory holding the single shared model copy.
    pub fn storage_dir() -> Result<PathBuf, String> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| "Could not resolve home directory for speech models.".to_string())?;
        Ok(PathBuf::from(home)
            .join(".observer_data")
            .join("models")
            .join("parakeet"))
    }

    pub fn marker_path(&self) -> Result<PathBuf, String> {
        let name = match self {
            SpeechModelId::ParakeetV2 => "v2.ready",
            SpeechModelId::ParakeetV3 => "v3.ready",
        };
        Ok(Self::storage_dir()?.join(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_supports_polish_v2_does_not() {
        assert!(SpeechModelId::ParakeetV3.supports_polish());
        assert!(!SpeechModelId::ParakeetV2.supports_polish());
    }

    #[test]
    fn marker_paths_differ_per_model() {
        assert_ne!(
            SpeechModelId::ParakeetV2.marker_path().unwrap(),
            SpeechModelId::ParakeetV3.marker_path().unwrap()
        );
    }
}
