use super::state::AppState;
use crate::core::multimodal::input_watch::{
    pinned_name, resolve_active, switch_message, ActiveInput, InputSnapshot,
};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// The microphone recording was last using, so a report that changes nothing
/// stays silent.
static ACTIVE: Mutex<Option<ActiveInput>> = Mutex::new(None);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InputChanged {
    name: String,
    message: String,
    falling_back: bool,
}

/// Core Audio reported the devices that exist: follow the change.
///
/// The dictation helper owns the Core Audio listener and sends this whenever a
/// device is added, removed, or made the system default, so nothing polls.
pub async fn handle_input_devices(app: AppHandle, snapshot: InputSnapshot) {
    let Some(state) = app.try_state::<AppState>() else { return };
    // Every report, even one that doesn't change the active microphone, so
    // microphone pickers can refresh their list without checking on a timer.
    let _ = app.emit("audio-inputs-changed", ());
    let pinned = state
        .config
        .lock()
        .ok()
        .and_then(|config| pinned_name(config.selected_audio_input_id.as_deref()));

    let current = resolve_active(pinned.as_deref(), &snapshot);
    let previous = {
        let Ok(mut active) = ACTIVE.lock() else { return };
        active.replace(current.clone())
    }
    .unwrap_or(ActiveInput { name: None, falling_back: false });

    if previous == current {
        return;
    }
    let (Some(message), Some(name)) = (
        switch_message(&previous, &current, pinned.as_deref()),
        current.name.clone(),
    ) else {
        return;
    };
    println!("[audio-input] {message}");

    restart_audio_capture(&state).await;
    if let Err(error) = app.emit(
        "audio-input-changed",
        InputChanged { name, message, falling_back: current.falling_back },
    ) {
        eprintln!("[audio-input] could not notify the window: {error}");
    }
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
