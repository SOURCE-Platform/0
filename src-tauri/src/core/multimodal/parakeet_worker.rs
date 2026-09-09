use super::audio_runtime::{
    ensure_audio_python, helper_dir, write_helper_if_needed, ParakeetTranscription,
};
use super::foreground_coordinator::wait_for_background_transcription;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

struct ParakeetWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

static WORKER: OnceLock<Mutex<Option<ParakeetWorker>>> = OnceLock::new();

pub(super) async fn transcribe(audio_path: &Path) -> Result<ParakeetTranscription, String> {
    wait_for_background_transcription().await;
    let worker = WORKER.get_or_init(|| Mutex::new(None));
    let mut guard = worker.lock().await;
    wait_for_background_transcription().await;
    if guard.is_none() {
        *guard = Some(start_worker().await?);
    }

    let result = request(guard.as_mut().expect("worker was initialized"), audio_path).await;
    if result.is_err() {
        if let Some(mut worker) = guard.take() {
            let _ = worker.child.kill().await;
        }
    }
    result
}

async fn start_worker() -> Result<ParakeetWorker, String> {
    let python = ensure_audio_python().await?;
    let script = ensure_runtime_file()?;
    let mut child = Command::new(python)
        .arg(script)
        .arg("--serve")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start local Parakeet worker: {error}"))?;
    let stdin = child.stdin.take().ok_or("Parakeet worker has no stdin.")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Parakeet worker has no stdout.")?;
    Ok(ParakeetWorker {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}

async fn request(
    worker: &mut ParakeetWorker,
    audio_path: &Path,
) -> Result<ParakeetTranscription, String> {
    let payload = serde_json::json!({ "audioPath": audio_path });
    worker
        .stdin
        .write_all(format!("{payload}\n").as_bytes())
        .await
        .map_err(|error| format!("Failed to send audio to Parakeet worker: {error}"))?;
    worker
        .stdin
        .flush()
        .await
        .map_err(|error| format!("Failed to flush Parakeet input: {error}"))?;
    let mut line = String::new();
    let read = worker
        .stdout
        .read_line(&mut line)
        .await
        .map_err(|error| format!("Failed to read Parakeet output: {error}"))?;
    if read == 0 {
        return Err("Parakeet worker ended before returning a transcript.".to_string());
    }
    serde_json::from_str(line.trim())
        .map_err(|error| format!("Failed to parse Parakeet output: {error}"))
}

fn ensure_runtime_file() -> Result<PathBuf, String> {
    let helpers_dir = helper_dir()?;
    std::fs::create_dir_all(&helpers_dir)
        .map_err(|error| format!("Failed to create helpers dir: {error}"))?;
    let script = helpers_dir.join("parakeet_runtime.py");
    write_helper_if_needed(&script, include_str!("../helpers/parakeet_runtime.py"))?;
    Ok(script)
}
