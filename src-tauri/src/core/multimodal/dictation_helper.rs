use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::broadcast;

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

#[derive(Debug, Clone)]
pub enum DictationEvent {
    Ready,
    SessionStarted { id: String },
    SessionStopped { id: String },
    Transcript(DictationTranscript),
    Inserted { id: String },
    Debug(String),
    EngineError(String),
    Exited,
}

pub struct DictationHelper {
    child: Child,
    stdin: ChildStdin,
    events: broadcast::Sender<DictationEvent>,
}

impl DictationHelper {
    /// Dev builds: absolute OUT_DIR path baked in at compile time.
    /// Packaged app: `Contents/Resources/helpers/source-dictation`
    /// next to the bundle (resolved from the running executable).
    pub fn helper_path() -> Option<PathBuf> {
        // Local override for live testing (e.g. point dev at the full
        // SwiftPM engine build instead of the swiftc stub).
        if let Ok(path) = std::env::var("SOURCE_DICTATION_HELPER_OVERRIDE") {
            let path = PathBuf::from(path);
            if path.exists() {
                return Some(path);
            }
        }
        if let Some(path) = option_env!("SOURCE_DICTATION_HELPER").map(PathBuf::from) {
            if path.exists() {
                return Some(path);
            }
        }
        bundled_helper_path()
    }

    pub fn available() -> bool {
        let available = Self::helper_path().is_some_and(|path| path.exists());
        if !available {
            eprintln!("Dictation helper binary not found (dev OUT_DIR or bundle Resources)");
        }
        available
    }

    /// Spawn `source-dictation serve <parent-pid>` and stream its stdout
    /// lines. Returns the handle plus a receiver for helper events.
    pub async fn spawn() -> Result<(Self, broadcast::Receiver<DictationEvent>), String> {
        let path = Self::helper_path()
            .ok_or_else(|| "Dictation helper is not bundled on this platform.".to_string())?;
        if !path.exists() {
            return Err("Dictation helper binary is missing.".to_string());
        }
        let parent_pid = std::process::id().to_string();
        let mut child = Command::new(&path)
            .arg("serve")
            .arg(parent_pid)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("Failed to start dictation helper: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or("Dictation helper has no stdin.")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Dictation helper has no stdout.")?;
        let (events, receiver) = broadcast::channel(64);
        let reader_events = events.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let event = parse_helper_line(&line);
                        let _ = reader_events.send(event.clone());
                        if matches!(event, DictationEvent::Exited) {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = reader_events.send(DictationEvent::Exited);
                        break;
                    }
                    Err(error) => {
                        let _ = reader_events
                            .send(DictationEvent::EngineError(format!("helper io: {error}")));
                        break;
                    }
                }
            }
        });
        Ok((Self { child, stdin, events }, receiver))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DictationEvent> {
        self.events.subscribe()
    }

    pub async fn start_session(&mut self, foreground_id: &str) -> Result<(), String> {
        self.send_line(format!("START {foreground_id}")).await
    }

    pub async fn stop_session(&mut self) -> Result<(), String> {
        self.send_line("STOP".to_string()).await
    }

    /// Ask the helper to type `text` into the captured focus target.
    /// Phase 5 implements Accessibility/CGEvent/pasteboard insertion
    /// in Swift; the helper replies `INSERTED <id>` on success.
    pub async fn request_insertion(&mut self, id: &str, text: &str) -> Result<(), String> {
        let payload = serde_json::json!({ "id": id, "text": text }).to_string();
        self.send_line(format!("INSERT {payload}")).await
    }

    pub async fn shutdown(&mut self) -> Result<(), String> {
        let _ = self.send_line("SHUTDOWN".to_string()).await;
        self.child
            .kill()
            .await
            .map_err(|error| format!("Failed to stop dictation helper: {error}"))?;
        Ok(())
    }

    async fn send_line(&mut self, line: String) -> Result<(), String> {
        self.stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|error| format!("Failed to send to dictation helper: {error}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| format!("Failed to flush dictation helper: {error}"))?;
        Ok(())
    }
}

/// macOS .app layout: `Contents/MacOS/<exe>` → `Contents/Resources/...`.
fn bundled_helper_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let resources = exe.parent()?.join("../Resources").canonicalize().ok()?;
    let candidate = resources.join("helpers").join("source-dictation");
    candidate.exists().then_some(candidate)
}

fn parse_helper_line(line: &str) -> DictationEvent {
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
    if let Some(message) = trimmed.strip_prefix("DEBUG ") {
        return DictationEvent::Debug(message.to_string());
    }
    if let Some(payload) = trimmed.strip_prefix("TRANSCRIPT ") {
        return match serde_json::from_str::<DictationTranscript>(payload) {
            Ok(transcript) => DictationEvent::Transcript(transcript),
            Err(error) => DictationEvent::EngineError(format!("bad transcript: {error}")),
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
}
