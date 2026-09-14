use super::claude_cli::resume_args;
use super::driver_events::{DriverEvent, TurnState};
use super::stream_protocol::{encode_user_message, parse_line};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, watch};

/// How long a conversation stays with SOURCE after its last activity before it
/// is handed back to the Claude app.
pub const RELEASE_AFTER_QUIET: Duration = Duration::from_secs(5 * 60);

/// How to start the process for one conversation.
#[derive(Debug, Clone)]
pub struct DriverConfig {
    pub binary: PathBuf,
    pub session_id: String,
    pub cwd: PathBuf,
    pub permission_mode: Option<String>,
    pub release_after_quiet: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverError {
    Spawn(String),
    Write(String),
}

struct Running {
    child: Child,
    stdin: ChildStdin,
}

/// Continues one Claude Code conversation in a process SOURCE owns.
///
/// The process starts on the first message and is released after a quiet
/// period, so the conversation goes back to the Claude app when nobody is
/// talking to it. Messages sent while Claude is working join that work.
pub struct ClaudeDriver {
    config: DriverConfig,
    events: broadcast::Sender<DriverEvent>,
    running: tokio::sync::Mutex<Option<Running>>,
    turn: Mutex<TurnState>,
    activity: watch::Sender<u64>,
    pid: Mutex<Option<u32>>,
}

impl ClaudeDriver {
    pub fn new(config: DriverConfig) -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        let (activity, _) = watch::channel(0);
        Arc::new(Self {
            config,
            events,
            running: tokio::sync::Mutex::new(None),
            turn: Mutex::new(TurnState::default()),
            activity,
            pid: Mutex::new(None),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.config.session_id
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DriverEvent> {
        self.events.subscribe()
    }

    pub fn is_busy(&self) -> bool {
        self.turn.lock().map(|turn| turn.busy()).unwrap_or(false)
    }

    /// The process SOURCE is running for this conversation, if any.
    pub fn pid(&self) -> Option<u32> {
        self.pid.lock().ok().and_then(|pid| *pid)
    }

    /// Send a message into the conversation, starting the process if needed.
    pub async fn send(self: &Arc<Self>, text: &str) -> Result<DriverEvent, DriverError> {
        let mut running = self.running.lock().await;
        if running.as_mut().map_or(true, |r| matches!(r.child.try_wait(), Ok(Some(_)))) {
            *running = Some(self.spawn()?);
        }
        let process = running.as_mut().expect("started above");
        let line = format!("{}\n", encode_user_message(text));
        let written = async {
            process.stdin.write_all(line.as_bytes()).await?;
            process.stdin.flush().await
        };
        written.await.map_err(|error| DriverError::Write(error.to_string()))?;

        let announced = self.turn.lock().map(|mut turn| turn.on_sent()).unwrap_or(DriverEvent::Working);
        let _ = self.events.send(announced.clone());
        self.touch();
        Ok(announced)
    }

    /// Hand the conversation back: close the process's input so it exits
    /// cleanly, and stop it if it doesn't within a few seconds.
    pub async fn release(&self) {
        let Some(mut process) = self.running.lock().await.take() else { return };
        drop(process.stdin);
        if tokio::time::timeout(Duration::from_secs(10), process.child.wait()).await.is_err() {
            let _ = process.child.kill().await;
        }
        // Don't wait for the output reader to notice: callers check this right away.
        if let Ok(mut pid) = self.pid.lock() {
            *pid = None;
        }
    }

    fn spawn(self: &Arc<Self>) -> Result<Running, DriverError> {
        let args = resume_args(&self.config.session_id, self.config.permission_mode.as_deref());
        let mut child = Command::new(&self.config.binary)
            .args(&args)
            .current_dir(&self.config.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(log_file().map(std::process::Stdio::from).unwrap_or_else(std::process::Stdio::null))
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| DriverError::Spawn(format!("{}: {error}", self.config.binary.display())))?;
        let stdin = child.stdin.take().ok_or_else(|| DriverError::Spawn("no stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| DriverError::Spawn("no stdout".into()))?;
        if let Ok(mut pid) = self.pid.lock() {
            *pid = child.id();
        }
        tokio::spawn(read_output(Arc::downgrade(self), stdout));
        tokio::spawn(release_when_quiet(Arc::downgrade(self), self.activity.subscribe()));
        Ok(Running { child, stdin })
    }

    fn touch(&self) {
        self.activity.send_modify(|count| *count += 1);
    }
}

async fn read_output(driver: Weak<ClaudeDriver>, stdout: tokio::process::ChildStdout) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Some(driver) = driver.upgrade() else { return };
        for event in parse_line(&line) {
            let translated = driver.turn.lock().ok().and_then(|mut turn| turn.on_stream(event));
            if let Some(event) = translated {
                let _ = driver.events.send(event);
            }
        }
        driver.touch();
    }
    // Output ended: the process exited (released, crashed, or signed out).
    if let Some(driver) = driver.upgrade() {
        if let Ok(mut turn) = driver.turn.lock() {
            turn.on_exit();
        }
        if let Ok(mut pid) = driver.pid.lock() {
            *pid = None;
        }
        let _ = driver.events.send(DriverEvent::Released);
    }
}

/// Release after a quiet period. Waits on a deadline that each bit of activity
/// pushes back, so nothing wakes up while the conversation is in use.
async fn release_when_quiet(driver: Weak<ClaudeDriver>, mut activity: watch::Receiver<u64>) {
    loop {
        let quiet = match driver.upgrade() {
            Some(driver) => driver.config.release_after_quiet,
            None => return,
        };
        tokio::select! {
            changed = activity.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            _ = tokio::time::sleep(quiet) => {
                let Some(driver) = driver.upgrade() else { return };
                if driver.is_busy() {
                    continue;
                }
                driver.release().await;
                return;
            }
        }
    }
}

fn log_file() -> Option<std::fs::File> {
    let home = std::env::var("HOME").ok()?;
    let dir = PathBuf::from(home).join(".observer_data").join("helpers");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("claude-driver.log"))
        .ok()
}
