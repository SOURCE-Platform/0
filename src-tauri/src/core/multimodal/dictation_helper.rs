use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::broadcast;

pub use super::dictation_helper_protocol::{DictationEvent, DictationTranscript, TranscribeFileRequest};
use super::dictation_helper_protocol::parse_helper_line;

pub struct DictationHelper {
    child: Child,
    stdin: ChildStdin,
    events: broadcast::Sender<DictationEvent>,
}

impl DictationHelper {
    /// Packaged app: `Contents/Resources/helpers/source-dictation`
    /// next to the bundle (resolved from the running executable).
    /// The bundled copy wins over the baked dev path so the running
    /// identity is stable: macOS TCC approvals (Microphone,
    /// Accessibility) are granted per binary location.
    pub fn helper_path() -> Option<PathBuf> {
        // Local override for live testing (e.g. point dev at the full
        // SwiftPM engine build instead of the swiftc stub).
        if let Ok(path) = std::env::var("SOURCE_DICTATION_HELPER_OVERRIDE") {
            let path = PathBuf::from(path);
            if path.exists() {
                return Some(path);
            }
        }
        if let Some(path) = bundled_helper_path() {
            return Some(path);
        }
        if let Some(path) = option_env!("SOURCE_DICTATION_HELPER").map(PathBuf::from) {
            if path.exists() {
                return Some(path);
            }
        }
        None
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
        let protocol_log = open_protocol_log();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        append_protocol_line(&protocol_log, &line);
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

    /// Ask the helper to transcribe an audio file on disk (e.g. a mobile
    /// clip spooled by the Source Mobile transport). The Swift side accepts
    /// either a bare path (legacy, keeps the verified path unchanged) or a
    /// JSON payload carrying id/source/timestamps.
    pub async fn transcribe_file(&mut self, req: TranscribeFileRequest) -> Result<(), String> {
        let payload = serde_json::to_string(&req)
            .map_err(|error| format!("Failed to encode transcribe request: {error}"))?;
        self.send_line(format!("TRANSCRIBE_FILE {payload}")).await
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

/// Mirror of the helper line protocol at
/// `~/.observer_data/helpers/dictation-helper.log` (truncated past
/// 512 KiB). The bundled app's stderr is otherwise invisible, which
/// makes hotkey/transcription failures undiagnosable in the field.
fn open_protocol_log() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir = PathBuf::from(home).join(".observer_data").join("helpers");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("dictation-helper.log");
    if let Ok(metadata) = std::fs::metadata(&path) {
        if metadata.len() > 512 * 1024 {
            let _ = std::fs::write(&path, "");
        }
    }
    Some(path)
}

fn append_protocol_line(path: &Option<PathBuf>, line: &str) {
    let Some(path) = path else { return };
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}
