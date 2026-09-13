use super::state::AppState;
use crate::core::multimodal::input_watch::{
    pinned_name, read_snapshot, resolve_active, switch_message, ActiveInput,
};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// How often to ask Core Audio what is plugged in. Cheap, and two seconds is
/// fast enough that a dictation press right after docking lands on the new mic.
const POLL: Duration = Duration::from_secs(2);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InputChanged {
    name: String,
    message: String,
    falling_back: bool,
}

/// Follow microphone changes: when the chosen mic is plugged in or pulled out,
/// move recording onto whatever is actually there and tell the user.
pub fn spawn_audio_input_watch(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut previous = ActiveInput { name: None, falling_back: false };
        loop {
            tokio::time::sleep(POLL).await;

            let snapshot = match tauri::async_runtime::spawn_blocking(read_snapshot).await {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    eprintln!("[audio-input] could not read input devices: {error}");
                    continue;
                }
            };

            let Some(state) = app.try_state::<AppState>() else { continue };
            let pinned = state
                .config
                .lock()
                .ok()
                .and_then(|config| pinned_name(config.selected_audio_input_id.as_deref()));

            let current = resolve_active(pinned.as_deref(), &snapshot);
            if current == previous {
                continue;
            }
            let message = switch_message(&previous, &current, pinned.as_deref());
            previous = current.clone();

            let (Some(name), Some(message)) = (current.name.clone(), message) else { continue };
            println!("[audio-input] {message}");

            restart_audio_capture(&state).await;
            if let Err(error) = app.emit(
                "audio-input-changed",
                InputChanged { name, message, falling_back: current.falling_back },
            ) {
                eprintln!("[audio-input] could not notify the window: {error}");
            }
        }
    });
}

/// Ambient capture binds its microphone when the channel starts, so the new
/// device only takes effect once the audio channel is started again. Dictation
/// needs no restart: it resolves the microphone per press.
async fn restart_audio_capture(state: &tauri::State<'_, AppState>) {
    let (is_active, session_id) = {
        let runtime = state.desktop_capture_runtime.read().await;
        (runtime.is_active, runtime.session_id.clone())
    };
    let (true, Some(session_id)) = (is_active, session_id) else { return };
    let Ok(config) = state.config.lock().map(|config| config.clone()) else { return };

    let mut started = false;
    crate::app::capture::sampler::start_multimodal_capture(state, &session_id, &config, &mut started)
        .await;
}
