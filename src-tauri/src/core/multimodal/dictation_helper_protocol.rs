//! The line protocol spoken by the bundled dictation helper: the events it
//! prints on stdout and how each line is parsed.
//!
//! Kept apart from the process management in `dictation_helper.rs` so both stay
//! readable, and so new protocol lines only touch this file.

/// Contract event published by the bundled dictation helper.
/// Matches `docs/transcription-architecture.md`: text, timestamps,
/// stable id, source `fluid-voice-prompt`, finality flag.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationTranscript {
    pub id: String,
    pub text: String,
    pub language: Option<String>,
    pub confidence: Option<f32>,
    pub provider: String,
    pub model: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub source: String,
    pub is_final: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeFileRequest {
    pub id: String,
    pub path: String,
    pub source: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
}

#[derive(Debug, Clone)]
pub enum DictationEvent {
    Ready,
    SessionStarted { id: String },
    SessionStopped { id: String },
    Transcript(DictationTranscript),
    Inserted { id: String },
    OpenSettings,
    Debug(String),
    /// Core Audio reported the input devices that exist right now.
    InputDevices { default_name: Option<String>, devices: Vec<String> },
    EngineError(String),
    Exited,
}

pub(super) fn parse_helper_line(line: &str) -> DictationEvent {
    let trimmed = line.trim();
    if trimmed == "READY" {
        return DictationEvent::Ready;
    }
    if let Some(id) = trimmed.strip_prefix("SESSION_STARTED ") {
        return DictationEvent::SessionStarted { id: id.trim().to_string() };
    }
    if let Some(id) = trimmed.strip_prefix("SESSION_STOPPED ") {
        return DictationEvent::SessionStopped { id: id.trim().to_string() };
    }
    if let Some(id) = trimmed.strip_prefix("INSERTED ") {
        return DictationEvent::Inserted { id: id.trim().to_string() };
    }
    if trimmed == "OPEN_SETTINGS" {
        return DictationEvent::OpenSettings;
    }
    if let Some(message) = trimmed.strip_prefix("DEBUG ") {
        return DictationEvent::Debug(message.to_string());
    }
    if let Some(payload) = trimmed.strip_prefix("TRANSCRIPT ") {
        return match serde_json::from_str::<DictationTranscript>(payload) {
            Ok(transcript) => DictationEvent::Transcript(transcript),
            Err(error) => DictationEvent::EngineError(format!("bad transcript: {error}")),
        };
    }
    if let Some(payload) = trimmed.strip_prefix("INPUT_DEVICES ") {
        #[derive(serde::Deserialize)]
        struct Payload {
            #[serde(default)]
            default: Option<String>,
            #[serde(default)]
            devices: Vec<String>,
        }
        return match serde_json::from_str::<Payload>(payload) {
            Ok(parsed) => DictationEvent::InputDevices {
                default_name: parsed.default,
                devices: parsed.devices,
            },
            Err(error) => DictationEvent::EngineError(format!("bad input devices: {error}")),
        };
    }
    if let Some(payload) = trimmed.strip_prefix("ERROR ") {
        return DictationEvent::EngineError(payload.to_string());
    }
    DictationEvent::EngineError(format!("unknown helper line: {trimmed}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ready() {
        assert!(matches!(parse_helper_line("READY"), DictationEvent::Ready));
    }

    #[test]
    fn parses_transcript_event() {
        let line = r#"TRANSCRIPT {"id":"abc","text":"hello","language":null,"confidence":0.9,"provider":"helper","model":"parakeet-tdt-v3","startedAtMs":1,"endedAtMs":2,"source":"fluid-voice-prompt","isFinal":true}"#;
        match parse_helper_line(line) {
            DictationEvent::Transcript(transcript) => {
                assert_eq!(transcript.id, "abc");
                assert_eq!(transcript.text, "hello");
                assert!(transcript.is_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn parses_session_events() {
        match parse_helper_line("SESSION_STARTED abc-123") {
            DictationEvent::SessionStarted { id } => assert_eq!(id, "abc-123"),
            other => panic!("unexpected event: {other:?}"),
        }
        match parse_helper_line("SESSION_STOPPED abc-123") {
            DictationEvent::SessionStopped { id } => assert_eq!(id, "abc-123"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn parses_insertion_ack() {
        match parse_helper_line("INSERTED abc-123") {
            DictationEvent::Inserted { id } => assert_eq!(id, "abc-123"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn parses_debug_lines() {
        assert!(matches!(
            parse_helper_line("DEBUG key flagsChanged keyCode=61 alt=true"),
            DictationEvent::Debug(_)
        ));
    }

    #[test]
    fn bad_transcript_becomes_engine_error() {
        assert!(matches!(
            parse_helper_line("TRANSCRIPT {oops"),
            DictationEvent::EngineError(_)
        ));
    }

    #[test]
    fn unknown_line_becomes_engine_error() {
        assert!(matches!(
            parse_helper_line("HELLO"),
            DictationEvent::EngineError(_)
        ));
    }

    #[test]
    fn parses_a_core_audio_device_report() {
        let event = parse_helper_line(
            r#"INPUT_DEVICES {"default":"MacBook Air Microphone","devices":["MacBook Air Microphone","USB-C Adapter"]}"#,
        );
        match event {
            DictationEvent::InputDevices { default_name, devices } => {
                assert_eq!(default_name.as_deref(), Some("MacBook Air Microphone"));
                assert_eq!(devices, ["MacBook Air Microphone", "USB-C Adapter"]);
            }
            other => panic!("expected an input device report, got {other:?}"),
        }
    }

    #[test]
    fn reports_a_missing_default_input_without_failing() {
        let event = parse_helper_line(r#"INPUT_DEVICES {"default":null,"devices":[]}"#);
        match event {
            DictationEvent::InputDevices { default_name, devices } => {
                assert!(default_name.is_none());
                assert!(devices.is_empty());
            }
            other => panic!("expected an input device report, got {other:?}"),
        }
    }
}
