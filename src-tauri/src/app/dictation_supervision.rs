use crate::core::config::Config;
use crate::core::database::Database;
use crate::core::multimodal::foreground_coordinator::set_background_transcription_paused;
use crate::core::multimodal::speech_provider::DictionaryEntry;
use crate::core::multimodal::{
    persist_foreground_transcript, DictationHelper, DictationSupervisor, PipelineAction,
    SupervisorCommand,
};
use crate::core::session_manager::SessionManager;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::sync::mpsc;

pub fn initialize_dictation_supervisor(
    config: Arc<Mutex<Config>>,
    db: Arc<Database>,
    session_manager: Option<Arc<SessionManager>>,
    app_handle: tauri::AppHandle,
    commands_holder: Arc<tokio::sync::Mutex<Option<mpsc::Sender<SupervisorCommand>>>>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            if !DictationHelper::available() {
                eprintln!("Dictation helper not available; retrying soon");
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                continue;
            }
            let dictionary: Vec<DictionaryEntry> = config
                .lock()
                .map(|config| {
                    config
                        .custom_dictionary
                        .iter()
                        .map(|entry| DictionaryEntry {
                            triggers: entry.triggers.clone(),
                            replacement: entry.replacement.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let supervisor = DictationSupervisor::new(dictionary);
            match supervisor.run().await {
                Ok((mut actions, _events, commands)) => {
                    println!("Dictation helper is ready");
                    *commands_holder.lock().await = Some(commands.clone());
                    while let Some(action) = actions.recv().await {
                        log_dictation_action(&action);
                        // Timeline first, typing second: a typing failure
                        // must never lose the captured transcript.
                        if let PipelineAction::PersistForeground {
                            id,
                            text,
                            source,
                            started_at_ms,
                            ended_at_ms,
                            language,
                            confidence,
                            model,
                        } = &action
                        {
                            persist_action(
                                &db,
                                &session_manager,
                                id,
                                text,
                                source,
                                *started_at_ms,
                                *ended_at_ms,
                                language.as_deref(),
                                *confidence,
                                model,
                            )
                            .await;
                        }
                        if let PipelineAction::InsertIntoFocusedField { id, text } = &action {
                            let command = SupervisorCommand::Insert(id.clone(), text.clone());
                            if commands.send(command).await.is_err() {
                                break;
                            }
                        }
                        if matches!(action, PipelineAction::OpenSettings) {
                            open_dictation_settings(&app_handle);
                        }
                    }
                    set_background_transcription_paused(false);
                    eprintln!("Dictation helper exited; restarting soon");
                }
                Err(error) => {
                    set_background_transcription_paused(false);
                    eprintln!("Dictation helper failed to start ({error}); retrying soon");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

/// Gear clicked on the dictation pill: bring O forward and jump the
/// interface straight to the dictation settings.
fn open_dictation_settings(app_handle: &tauri::AppHandle) {
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Err(error) = app_handle.emit("dictation-open-settings", ()) {
        eprintln!("Failed to open dictation settings: {error}");
    }
}

async fn persist_action(
    db: &Arc<Database>,
    session_manager: &Option<Arc<SessionManager>>,
    id: &str,
    text: &str,
    source: &str,
    started_at_ms: i64,
    ended_at_ms: i64,
    language: Option<&str>,
    confidence: Option<f32>,
    model: &str,
) {
    let mut session_id = "dictation".to_string();
    if let Some(manager) = session_manager {
        match manager.get_current_session().await {
            Ok(Some(session)) => session_id = session.id,
            Ok(None) => {}
            Err(error) => eprintln!("Dictation session lookup failed: {error}"),
        }
    }
    // Remote captures own their own lane. Writing them through the dictation
    // store would stamp them `fluid-voice-prompt` and collide with the
    // placeholder row already holding this id, silently dropping the text.
    let result = if source == crate::core::multimodal::MOBILE_SOURCE_ID {
        crate::core::multimodal::persist_mobile_transcript(
            db,
            &session_id,
            id,
            text,
            started_at_ms,
            ended_at_ms,
            language,
            confidence,
            model,
            true,
        )
        .await
    } else {
        persist_foreground_transcript(
            db,
            &session_id,
            id,
            text,
            started_at_ms,
            ended_at_ms,
            language,
            confidence,
            model,
        )
        .await
    };
    match result {
        Err(error) => eprintln!("Failed to save {source} transcript {id}: {error}"),
        Ok(()) => println!("Transcript {id} ({source}) saved to timeline"),
    }
}

fn log_dictation_action(action: &PipelineAction) {
    match action {
        PipelineAction::PersistForeground { id, text, .. } => {
            println!("Dictation transcript {id}: {text}");
        }
        PipelineAction::InsertIntoFocusedField { id, .. } => {
            println!("Dictation {id} ready to type into focused field");
        }
        PipelineAction::DuplicateIgnored { id } => {
            println!("Dictation duplicate {id} ignored");
        }
        PipelineAction::OpenSettings => {}
        PipelineAction::None => {}
    }
}
