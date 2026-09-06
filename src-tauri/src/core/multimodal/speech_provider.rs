use std::path::Path;

use async_trait::async_trait;

/// A single completed transcription unit with stable handoff fields.
/// Shared by background capture and foreground Right Option dictation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechTranscript {
    pub text: String,
    pub language: Option<String>,
    pub confidence: Option<f32>,
    pub provider: String,
    pub model: String,
    pub is_final: bool,
}

/// Which engine produced a transcript. Only one is resident at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpeechProviderKind {
    /// Current default: local Python mlx-audio worker.
    LocalMlx,
    /// Future: bundled Swift helper using FluidAudio / Core ML.
    NativeHelper,
    /// Future: FluidVoice sidecar over localhost, prototype only.
    SidecarApi,
}

/// Minimal provider contract so capture code never calls a worker directly.
#[async_trait]
pub trait SpeechProvider: Send + Sync {
    fn kind(&self) -> SpeechProviderKind;
    fn provider_name(&self) -> String;
    async fn transcribe_file(&self, audio_path: &Path) -> Result<SpeechTranscript, String>;
}

/// Current default engine: the local Python mlx-audio worker.
/// Model identity comes from the shared catalog so background and
/// foreground paths report the same model string.
pub struct LocalMlxProvider {
    pub model: String,
}

impl LocalMlxProvider {
    pub fn parakeet_v3() -> Self {
        Self {
            model: "mlx-community/parakeet-tdt-0.6b-v3".to_string(),
        }
    }
}

#[async_trait]
impl SpeechProvider for LocalMlxProvider {
    fn kind(&self) -> SpeechProviderKind {
        SpeechProviderKind::LocalMlx
    }

    fn provider_name(&self) -> String {
        "local-mlx".to_string()
    }

    async fn transcribe_file(&self, audio_path: &Path) -> Result<SpeechTranscript, String> {
        let raw = super::parakeet_worker::transcribe(audio_path).await?;
        Ok(SpeechTranscript {
            text: raw.text,
            language: raw.language,
            confidence: raw.confidence,
            provider: self.provider_name(),
            model: self.model.clone(),
            is_final: true,
        })
    }
}

/// One user dictionary rule: when `trigger` is heard, write `replacement`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub triggers: Vec<String>,
    pub replacement: String,
}

pub fn sanitize_replacement(value: &str) -> String {
    value.trim().to_string()
}

pub fn normalize_trigger(value: &str) -> Option<String> {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    Some(normalized)
}

/// Runtime-agnostic post-processing: swap misheard triggers for replacements.
/// Case-insensitive, longest trigger wins at each position. Single left-to-right
/// pass so replacement text is never re-matched (e.g. "rick" must not fire
/// inside the "Kubrick" we just inserted).
pub fn apply_dictionary_entries(text: &str, entries: &[DictionaryEntry]) -> String {
    struct Rule {
        lower: String,
        chars: usize,
        replacement: String,
    }
    let mut rules: Vec<Rule> = Vec::new();
    for entry in entries {
        let replacement = entry.replacement.trim();
        if replacement.is_empty() {
            continue;
        }
        for trigger in &entry.triggers {
            let trigger = trigger.trim();
            if trigger.is_empty() {
                continue;
            }
            rules.push(Rule {
                lower: trigger.to_lowercase(),
                chars: trigger.chars().count(),
                replacement: replacement.to_string(),
            });
        }
    }
    rules.sort_by(|a, b| b.lower.len().cmp(&a.lower.len()));

    let chars: Vec<char> = text.chars().collect();
    let byte_offsets: Vec<usize> = {
        let mut offsets: Vec<usize> =
            text.char_indices().map(|(index, _)| index).collect();
        offsets.push(text.len());
        offsets
    };
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let byte_at = byte_offsets[index];
        let rest = &text[byte_at..];
        let rest_lower = rest.to_lowercase();
        let mut matched: Option<&Rule> = None;
        for rule in &rules {
            if rest_lower.starts_with(rule.lower.as_str()) {
                matched = Some(rule);
                break;
            }
        }
        match matched {
            Some(rule) => {
                result.push_str(&rule.replacement);
                index += rule.chars;
            }
            None => {
                result.push(chars[index]);
                index += 1;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_misheard_trigger_case_insensitively() {
        let entries = vec![DictionaryEntry {
            triggers: vec!["cub rick".to_string()],
            replacement: "Kubrick".to_string(),
        }];
        assert_eq!(
            apply_dictionary_entries("met CUB RICK today", &entries),
            "met Kubrick today"
        );
    }

    #[test]
    fn longest_trigger_wins() {
        let entries = vec![
            DictionaryEntry {
                triggers: vec!["rick".to_string()],
                replacement: "WRONG".to_string(),
            },
            DictionaryEntry {
                triggers: vec!["cub rick".to_string()],
                replacement: "Kubrick".to_string(),
            },
        ];
        assert_eq!(apply_dictionary_entries("cub rick", &entries), "Kubrick");
    }

    #[test]
    fn empty_replacement_is_ignored() {
        let entries = vec![DictionaryEntry {
            triggers: vec!["hello".to_string()],
            replacement: "   ".to_string(),
        }];
        assert_eq!(apply_dictionary_entries("hello", &entries), "hello");
    }
}
