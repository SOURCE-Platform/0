use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use uuid::Uuid;

const DESKTOP_AUDIO_SAMPLE_RATE: u32 = 16_000;
const DESKTOP_CHUNK_TIMEOUT: Duration = Duration::from_secs(4);

static LIVE_DESKTOP_AUDIO: OnceLock<Mutex<Option<Arc<DesktopAudioStream>>>> = OnceLock::new();

struct DesktopAudioStream {
    level_bits: Arc<AtomicU32>,
    last_error: Arc<Mutex<Option<String>>>,
    capture_enabled: AtomicBool,
    chunks: Mutex<Receiver<PathBuf>>,
    child: Mutex<Child>,
    spool_directory: PathBuf,
    _reader: JoinHandle<()>,
}

impl Drop for DesktopAudioStream {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.spool_directory);
    }
}

pub(super) fn ensure_live_desktop_meter(_gain_db: f32) -> Result<f32, String> {
    let stream = ensure_live_desktop_stream()?;
    stream.current_level()
}

pub(super) fn current_live_desktop_meter(_gain_db: f32) -> Result<f32, String> {
    let stream = ensure_live_desktop_stream()?;
    stream.current_level()
}

pub(super) async fn capture_desktop_audio_chunk(
    _duration_secs: f32,
    gain_db: f32,
    output_path: &Path,
) -> Result<(), String> {
    let stream = ensure_live_desktop_stream()?;
    stream.enable_capture()?;

    let raw_path = tokio::task::spawn_blocking({
        let stream = Arc::clone(&stream);
        move || stream.next_chunk()
    })
    .await
    .map_err(|error| format!("Desktop audio chunk task failed: {error}"))??;

    let output_path = output_path.to_path_buf();
    tokio::task::spawn_blocking(move || convert_chunk(&raw_path, gain_db, &output_path))
        .await
        .map_err(|error| format!("Desktop audio conversion task failed: {error}"))?
}

pub(super) fn stop_live_desktop_capture() {
    let Some(stream) = LIVE_DESKTOP_AUDIO
        .get()
        .and_then(|active| active.lock().ok())
        .and_then(|active| active.clone())
    else {
        return;
    };
    let _ = stream.disable_capture();
}

fn ensure_live_desktop_stream() -> Result<Arc<DesktopAudioStream>, String> {
    let active = LIVE_DESKTOP_AUDIO.get_or_init(|| Mutex::new(None));
    let mut active = active
        .lock()
        .map_err(|_| "Desktop audio stream lock is unavailable.".to_string())?;

    if let Some(stream) = active.as_ref() {
        if stream.current_level().is_ok() {
            return Ok(Arc::clone(stream));
        }
    }

    *active = None;
    let stream = Arc::new(start_desktop_stream()?);
    *active = Some(Arc::clone(&stream));
    Ok(stream)
}

fn start_desktop_stream() -> Result<DesktopAudioStream, String> {
    let spool_directory =
        std::env::temp_dir().join(format!("source-desktop-audio-{}", Uuid::new_v4()));
    fs::create_dir_all(&spool_directory)
        .map_err(|error| format!("Could not create desktop audio spool: {error}"))?;

    let parent_pid = std::process::id().to_string();
    let mut child = Command::new(helper_path()?)
        .args([
            "stream",
            spool_directory.to_string_lossy().as_ref(),
            &parent_pid,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start desktop audio helper: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Desktop audio helper did not expose a signal stream.".to_string())?;

    let level_bits = Arc::new(AtomicU32::new(0f32.to_bits()));
    let last_error = Arc::new(Mutex::new(None));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (chunk_tx, chunk_rx) = mpsc::channel();
    let reader = spawn_stream_reader(
        stdout,
        Arc::clone(&level_bits),
        Arc::clone(&last_error),
        ready_tx,
        chunk_tx,
    );

    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(DesktopAudioStream {
            level_bits,
            last_error,
            capture_enabled: AtomicBool::new(false),
            chunks: Mutex::new(chunk_rx),
            child: Mutex::new(child),
            spool_directory,
            _reader: reader,
        }),
        Ok(Err(error)) => {
            terminate_child(&mut child);
            let _ = reader.join();
            let _ = fs::remove_dir_all(&spool_directory);
            Err(error)
        }
        Err(_) => {
            terminate_child(&mut child);
            let _ = reader.join();
            let _ = fs::remove_dir_all(&spool_directory);
            Err("Desktop audio helper did not become ready quickly enough.".to_string())
        }
    }
}

fn spawn_stream_reader(
    stdout: impl std::io::Read + Send + 'static,
    level_bits: Arc<AtomicU32>,
    last_error: Arc<Mutex<Option<String>>>,
    ready_tx: Sender<Result<(), String>>,
    chunk_tx: Sender<PathBuf>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut ready_sent = false;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line == "READY" {
                ready_sent = true;
                let _ = ready_tx.send(Ok(()));
            } else if let Some(level) = line.strip_prefix("LEVEL ") {
                if let Ok(level) = level.parse::<f32>() {
                    level_bits.store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                }
            } else if let Some(path) = line.strip_prefix("CHUNK ") {
                let _ = chunk_tx.send(PathBuf::from(path));
            } else if let Some(error) = line.strip_prefix("ERROR ") {
                store_error(&last_error, error.to_string());
                if !ready_sent {
                    let _ = ready_tx.send(Err(error.to_string()));
                    ready_sent = true;
                }
            }
        }
        if !ready_sent {
            let _ = ready_tx.send(Err(
                "Desktop audio helper exited before it was ready.".to_string()
            ));
        } else {
            store_error(
                &last_error,
                "Desktop audio stream stopped unexpectedly.".to_string(),
            );
        }
    })
}

impl DesktopAudioStream {
    fn current_level(&self) -> Result<f32, String> {
        if let Ok(error) = self.last_error.lock() {
            if let Some(error) = error.clone() {
                return Err(error);
            }
        }
        Ok(f32::from_bits(self.level_bits.load(Ordering::Relaxed)).clamp(0.0, 1.0))
    }

    fn enable_capture(&self) -> Result<(), String> {
        if self.capture_enabled.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.discard_queued_chunks();
        self.send_command("CAPTURE_START")
    }

    fn disable_capture(&self) -> Result<(), String> {
        if !self.capture_enabled.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        self.send_command("CAPTURE_STOP")?;
        self.discard_queued_chunks();
        Ok(())
    }

    fn next_chunk(&self) -> Result<PathBuf, String> {
        let chunks = self
            .chunks
            .lock()
            .map_err(|_| "Desktop audio chunk queue is unavailable.".to_string())?;
        chunks.recv_timeout(DESKTOP_CHUNK_TIMEOUT).map_err(|error| {
            format!("Desktop audio stream did not deliver a chunk in time: {error}")
        })
    }

    fn send_command(&self, command: &str) -> Result<(), String> {
        let mut child = self
            .child
            .lock()
            .map_err(|_| "Desktop audio helper process is unavailable.".to_string())?;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Desktop audio helper cannot accept capture commands.".to_string())?;
        writeln!(stdin, "{command}")
            .and_then(|_| stdin.flush())
            .map_err(|error| format!("Could not control desktop audio helper: {error}"))
    }

    fn discard_queued_chunks(&self) {
        let Ok(chunks) = self.chunks.lock() else {
            return;
        };
        while let Ok(path) = chunks.try_recv() {
            let _ = fs::remove_file(path);
        }
    }
}

fn convert_chunk(raw_path: &Path, gain_db: f32, output_path: &Path) -> Result<(), String> {
    let gain_db = gain_db.clamp(0.0, 24.0);
    let mut command = Command::new("ffmpeg");
    command.args([
        "-y",
        "-loglevel",
        "error",
        "-f",
        "f32le",
        "-ar",
        &DESKTOP_AUDIO_SAMPLE_RATE.to_string(),
        "-ac",
        "1",
        "-i",
        raw_path.to_string_lossy().as_ref(),
    ]);
    if gain_db > 0.0 {
        command.args(["-af", &format!("volume={gain_db}dB")]);
    }
    let output = command
        .arg(output_path)
        .output()
        .map_err(|error| format!("Could not convert desktop audio: {error}"))?;
    let _ = fs::remove_file(raw_path);
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Desktop audio conversion failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn store_error(last_error: &Mutex<Option<String>>, message: String) {
    if let Ok(mut stored) = last_error.lock() {
        *stored = Some(message);
    }
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn helper_path() -> Result<PathBuf, String> {
    option_env!("SOURCE_DESKTOP_AUDIO_HELPER")
        .map(PathBuf::from)
        .ok_or_else(|| "Desktop audio helper was not bundled into this build.".to_string())
}
