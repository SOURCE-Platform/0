use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use uuid::Uuid;

const DESKTOP_AUDIO_SAMPLE_RATE: u32 = 16_000;
static LIVE_DESKTOP_METER: OnceLock<Mutex<Option<DesktopMeterHandle>>> = OnceLock::new();
static LAST_DESKTOP_METER_FAILURE: OnceLock<Mutex<Option<(Instant, String)>>> = OnceLock::new();
const DESKTOP_METER_RETRY_DELAY: Duration = Duration::from_secs(2);

struct DesktopMeterHandle {
    level_bits: Arc<AtomicU32>,
    last_error: Arc<Mutex<Option<String>>>,
    child: Arc<Mutex<Child>>,
    _reader: JoinHandle<()>,
}

impl Drop for DesktopMeterHandle {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(super) fn ensure_live_desktop_meter(gain_db: f32) -> Result<f32, String> {
    let meter = LIVE_DESKTOP_METER.get_or_init(|| Mutex::new(None));
    let mut active = meter
        .lock()
        .map_err(|_| "Desktop audio meter lock is unavailable.".to_string())?;
    read_or_restart_desktop_meter(&mut active, gain_db)
}

pub(super) fn current_live_desktop_meter(gain_db: f32) -> Result<f32, String> {
    let meter = LIVE_DESKTOP_METER
        .get()
        .ok_or_else(|| "Desktop audio meter has not started.".to_string())?;
    let mut active = meter
        .lock()
        .map_err(|_| "Desktop audio meter lock is unavailable.".to_string())?;
    read_or_restart_desktop_meter(&mut active, gain_db)
}

pub(super) async fn capture_desktop_audio_chunk(
    duration_secs: f32,
    gain_db: f32,
    output_path: &Path,
) -> Result<(), String> {
    let output_path = output_path.to_path_buf();
    tokio::task::spawn_blocking(move || capture_chunk_blocking(duration_secs, gain_db, output_path))
        .await
        .map_err(|error| format!("Desktop audio capture task failed: {error}"))?
}

fn read_or_restart_desktop_meter(
    active: &mut Option<DesktopMeterHandle>,
    _gain_db: f32,
) -> Result<f32, String> {
    let stale = active
        .as_ref()
        .map(|meter| read_meter(meter).is_err())
        .unwrap_or(true);
    if stale {
        *active = None;
        if let Some(message) = recent_desktop_meter_failure() {
            return Err(format!("Desktop audio is reconnecting. {message}"));
        }
        match start_desktop_meter() {
            Ok(meter) => {
                clear_desktop_meter_failure();
                *active = Some(meter);
            }
            Err(error) => {
                record_desktop_meter_failure(error.clone());
                return Err(format!("Desktop audio is reconnecting. {error}"));
            }
        }
    }
    active
        .as_ref()
        .map(read_meter)
        .transpose()?
        .ok_or_else(|| "Desktop audio meter did not start.".to_string())
}

fn recent_desktop_meter_failure() -> Option<String> {
    let failures = LAST_DESKTOP_METER_FAILURE.get_or_init(|| Mutex::new(None));
    let mut failure = failures.lock().ok()?;
    let (timestamp, message) = failure.as_ref()?;
    if timestamp.elapsed() < DESKTOP_METER_RETRY_DELAY {
        return Some(message.clone());
    }
    *failure = None;
    None
}

fn record_desktop_meter_failure(message: String) {
    let failures = LAST_DESKTOP_METER_FAILURE.get_or_init(|| Mutex::new(None));
    if let Ok(mut failure) = failures.lock() {
        *failure = Some((Instant::now(), message));
    }
}

fn clear_desktop_meter_failure() {
    let failures = LAST_DESKTOP_METER_FAILURE.get_or_init(|| Mutex::new(None));
    if let Ok(mut failure) = failures.lock() {
        *failure = None;
    }
}

fn start_desktop_meter() -> Result<DesktopMeterHandle, String> {
    let parent_pid = std::process::id().to_string();
    let mut child = Command::new(helper_path()?)
        // Keep the meter raw. Changing the user-facing gain must never restart
        // ScreenCaptureKit; the UI applies its sensitivity curve separately.
        .args(["meter", "0", &parent_pid])
        .stdout(Stdio::piped())
        // The helper reports actionable ScreenCaptureKit failures on stdout.
        // Discard stderr so an undrained pipe cannot stall the child.
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
    let reader = spawn_meter_reader(
        stdout,
        Arc::clone(&level_bits),
        Arc::clone(&last_error),
        ready_tx,
    );
    let child = Arc::new(Mutex::new(child));

    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(DesktopMeterHandle {
            level_bits,
            last_error,
            child,
            _reader: reader,
        }),
        Ok(Err(error)) => {
            terminate_meter_process(&child);
            let _ = reader.join();
            Err(error)
        }
        Err(_) => {
            terminate_meter_process(&child);
            let _ = reader.join();
            Err("Desktop audio helper did not become ready quickly enough.".to_string())
        }
    }
}

fn terminate_meter_process(child: &Arc<Mutex<Child>>) {
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn spawn_meter_reader(
    stdout: impl std::io::Read + Send + 'static,
    level_bits: Arc<AtomicU32>,
    last_error: Arc<Mutex<Option<String>>>,
    ready_tx: Sender<Result<(), String>>,
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
            } else if let Some(error) = line.strip_prefix("ERROR ") {
                let message = error.to_string();
                if let Ok(mut stored) = last_error.lock() {
                    *stored = Some(message.clone());
                }
                if !ready_sent {
                    let _ = ready_tx.send(Err(message));
                    ready_sent = true;
                }
            }
        }
        if !ready_sent {
            let _ = ready_tx.send(Err(
                "Desktop audio helper exited before it was ready.".to_string()
            ));
        } else if let Ok(mut stored) = last_error.lock() {
            *stored = Some("Desktop audio stream was stopped by macOS.".to_string());
        }
    })
}

fn capture_chunk_blocking(
    duration_secs: f32,
    gain_db: f32,
    output_path: PathBuf,
) -> Result<(), String> {
    let raw_path = std::env::temp_dir().join(format!("source-desktop-{}.f32", Uuid::new_v4()));
    let capture = Command::new(helper_path()?)
        .args([
            "capture",
            raw_path.to_string_lossy().as_ref(),
            &duration_secs.max(0.2).to_string(),
            &gain_db.clamp(0.0, 24.0).to_string(),
        ])
        .output()
        .map_err(|error| format!("Could not run desktop audio helper: {error}"))?;
    if !capture.status.success() {
        let details = String::from_utf8_lossy(&capture.stdout);
        let _ = fs::remove_file(&raw_path);
        return Err(format!("Desktop audio helper failed: {}", details.trim()));
    }
    if raw_path.metadata().map(|meta| meta.len()).unwrap_or(0) == 0 {
        let _ = fs::remove_file(&raw_path);
        return Err("Desktop audio helper received no audio samples.".to_string());
    }

    let conversion = Command::new("ffmpeg")
        .args([
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
            output_path.to_string_lossy().as_ref(),
        ])
        .output()
        .map_err(|error| format!("Could not convert desktop audio: {error}"))?;
    let _ = fs::remove_file(&raw_path);
    if conversion.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Desktop audio conversion failed: {}",
            String::from_utf8_lossy(&conversion.stderr).trim()
        ))
    }
}

fn read_meter(meter: &DesktopMeterHandle) -> Result<f32, String> {
    if let Ok(error) = meter.last_error.lock() {
        if let Some(error) = error.clone() {
            return Err(error);
        }
    }
    Ok(f32::from_bits(meter.level_bits.load(Ordering::Relaxed)).clamp(0.0, 1.0))
}

fn helper_path() -> Result<PathBuf, String> {
    option_env!("SOURCE_DESKTOP_AUDIO_HELPER")
        .map(PathBuf::from)
        .ok_or_else(|| "Desktop audio helper was not bundled into this build.".to_string())
}
