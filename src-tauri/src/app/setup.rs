use super::state::{AppState, DesktopCaptureRuntime};
use crate::core::config::Config;
use crate::core::consent::ConsentManager;
use crate::core::context_timeline;
use crate::core::database::Database;
use crate::core::multimodal::{
    persist_foreground_transcript, DictationHelper, DictationSupervisor, PipelineAction,
    SupervisorCommand,
};
use crate::core::input_recorder::InputRecorder;
use crate::core::keyboard_recorder::KeyboardRecorder;
use crate::core::multimodal::MultimodalService;
use crate::core::ocr_engine::{OcrConfig, OcrEngine};
use crate::core::ocr_processor::{OcrProcessor, OcrProcessorConfig};
use crate::core::ocr_storage::OcrStorage;
use crate::core::ocr_trigger_signals::OcrTriggerSignals;
use crate::core::os_activity::OsActivityRecorder;
use crate::core::playback_engine::PlaybackEngine;
use crate::core::screen_recorder::ScreenRecorder;
use crate::core::search_engine::SearchEngine;
use crate::core::session_manager::{SessionConfig, SessionManager};
use crate::core::storage::RecordingStorage;
use crate::platform::get_platform;
use std::sync::{Arc, Mutex};
use tauri::Emitter;
use tauri::Manager;
use tokio::sync::RwLock;

pub fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    tauri::async_runtime::block_on(async {
        let db = Arc::new(
            Database::init()
                .await
                .expect("Failed to initialize database"),
        );
        let consent_manager = Arc::new(
            ConsentManager::new(db.clone())
                .await
                .expect("Failed to initialize consent manager"),
        );

        let config = Config::load().expect("Failed to load configuration");
        context_timeline::init_schema(&db)
            .await
            .expect("Failed to initialize context timeline schema");

        let platform = get_platform();
        let data_dir = platform
            .get_data_directory()
            .expect("Failed to get data directory");
        let recordings_path = data_dir.join("recordings");
        let storage = Arc::new(
            RecordingStorage::new(recordings_path, db.clone())
                .await
                .expect("Failed to initialize recording storage"),
        );

        let ocr_processor = initialize_ocr_processor(&db, &config).await;
        let ocr_trigger_signals = Arc::new(OcrTriggerSignals::new());
        let screen_recorder = initialize_screen_recorder(
            consent_manager.clone(),
            storage.clone(),
            ocr_trigger_signals.clone(),
            ocr_processor.clone(),
        )
        .await;
        let os_activity_recorder =
            initialize_os_activity_recorder(consent_manager.clone(), db.clone()).await;

        if let (Some(screen_recorder), Some(os_activity_recorder)) =
            (screen_recorder.as_ref(), os_activity_recorder.as_ref())
        {
            screen_recorder
                .attach_os_activity_recorder(os_activity_recorder.clone())
                .await;
        }

        let session_manager = initialize_session_manager(db.clone()).await;
        let keyboard_recorder =
            initialize_keyboard_recorder(consent_manager.clone(), db.clone()).await;
        let input_recorder =
            initialize_input_recorder(consent_manager.clone(), db.clone(), ocr_trigger_signals)
                .await;
        let search_engine = Arc::new(SearchEngine::new(db.clone()));
        let playback_engine = Arc::new(PlaybackEngine::new(storage.clone(), db.clone()));
        let multimodal_service = Arc::new(MultimodalService::new(
            db.clone(),
            storage.clone(),
            consent_manager.clone(),
        ));
        let shared_config = Arc::new(Mutex::new(config));
        initialize_dictation_supervisor(
            shared_config.clone(),
            db.clone(),
            session_manager.clone(),
            app.handle().clone(),
        );

        app.manage(AppState {
            db,
            consent_manager,
            config: shared_config,
            screen_recorder,
            os_activity_recorder,
            session_manager,
            keyboard_recorder,
            input_recorder,
            search_engine,
            playback_engine: Some(playback_engine),
            storage: Some(storage),
            ocr_processor,
            multimodal_service: Some(multimodal_service),
            desktop_capture_runtime: Arc::new(RwLock::new(DesktopCaptureRuntime::default())),
        });
    });

    Ok(())
}

fn initialize_dictation_supervisor(
    config: Arc<Mutex<Config>>,
    db: Arc<Database>,
    session_manager: Option<Arc<SessionManager>>,
    app_handle: tauri::AppHandle,
) {
    use crate::core::multimodal::speech_provider::DictionaryEntry;
    use crate::core::multimodal::persist_foreground_transcript;

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
                    while let Some(action) = actions.recv().await {
                        log_dictation_action(&action);
                        // Timeline first, typing second: a typing failure
                        // must never lose the captured transcript.
                        if let PipelineAction::PersistForeground {
                            id,
                            text,
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
                                *started_at_ms,
                                *ended_at_ms,
                                language.as_deref(),
                                *confidence,
                                model,
                            )
                            .await;
                        }
                        if let PipelineAction::InsertIntoFocusedField { id, text } = &action
                        {
                            let command =
                                SupervisorCommand::Insert(id.clone(), text.clone());
                            if commands.send(command).await.is_err() {
                                break;
                            }
                        }
                        if matches!(action, PipelineAction::OpenSettings) {
                            open_dictation_settings(&app_handle);
                        }
                    }
                    eprintln!("Dictation helper exited; restarting soon");
                }
                Err(error) => {
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
    use tauri::Manager;
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
    if let Err(error) = persist_foreground_transcript(
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
    {
        eprintln!("{error}");
    } else {
        println!("Dictation {id} saved to timeline");
    }
}

/// Phase B: visibility into the live loop. Phase D persists these.
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

async fn initialize_ocr_processor(
    db: &Arc<Database>,
    config: &Config,
) -> Option<Arc<OcrProcessor>> {
    match OcrEngine::new(OcrConfig {
        languages: config.ocr_languages.clone(),
        confidence_threshold: config.ocr_confidence_threshold,
        ..OcrConfig::for_screenshots()
    }) {
        Ok(engine) => {
            let processor = Arc::new(OcrProcessor::new(
                Arc::new(engine),
                Arc::new(OcrStorage::new(db.clone())),
                OcrProcessorConfig {
                    enabled: true,
                    interval_seconds: config.ocr_interval_seconds,
                    ..OcrProcessorConfig::default()
                },
            ));
            if let Err(error) = processor.start().await {
                eprintln!("Warning: Failed to start OCR processor: {}", error);
                None
            } else {
                println!("OCR processor initialized successfully");
                Some(processor)
            }
        }
        Err(error) => {
            eprintln!("Warning: Failed to initialize OCR engine: {}", error);
            eprintln!("OCR capture will be unavailable until Tesseract initializes cleanly");
            None
        }
    }
}

async fn initialize_screen_recorder(
    consent_manager: Arc<ConsentManager>,
    storage: Arc<RecordingStorage>,
    ocr_trigger_signals: Arc<OcrTriggerSignals>,
    ocr_processor: Option<Arc<OcrProcessor>>,
) -> Option<ScreenRecorder> {
    match ScreenRecorder::new(consent_manager, storage, ocr_trigger_signals).await {
        Ok(recorder) => {
            if let Some(ocr_processor) = ocr_processor {
                recorder.attach_ocr_processor(ocr_processor).await;
            }
            println!("Screen recorder initialized successfully");
            Some(recorder)
        }
        Err(error) => {
            eprintln!("Warning: Failed to initialize screen recorder: {}", error);
            eprintln!("Screen recording features will be unavailable");
            None
        }
    }
}

async fn initialize_os_activity_recorder(
    consent_manager: Arc<ConsentManager>,
    db: Arc<Database>,
) -> Option<Arc<OsActivityRecorder>> {
    match OsActivityRecorder::new(consent_manager, db).await {
        Ok(recorder) => {
            println!("OS activity recorder initialized successfully");
            Some(Arc::new(recorder))
        }
        Err(error) => {
            eprintln!(
                "Warning: Failed to initialize OS activity recorder: {}",
                error
            );
            eprintln!("OS activity monitoring features will be unavailable");
            None
        }
    }
}

async fn initialize_session_manager(db: Arc<Database>) -> Option<Arc<SessionManager>> {
    match SessionManager::new(db, SessionConfig::default()).await {
        Ok(manager) => {
            println!("Session manager initialized successfully");
            Some(Arc::new(manager))
        }
        Err(error) => {
            eprintln!("Warning: Failed to initialize session manager: {}", error);
            eprintln!("Session management features will be unavailable");
            None
        }
    }
}

async fn initialize_keyboard_recorder(
    consent_manager: Arc<ConsentManager>,
    db: Arc<Database>,
) -> Option<Arc<KeyboardRecorder>> {
    match KeyboardRecorder::new(consent_manager, db).await {
        Ok(recorder) => {
            println!("Keyboard recorder initialized successfully");
            Some(Arc::new(recorder))
        }
        Err(error) => {
            eprintln!("Warning: Failed to initialize keyboard recorder: {}", error);
            eprintln!("Keyboard recording features will be unavailable");
            None
        }
    }
}

async fn initialize_input_recorder(
    consent_manager: Arc<ConsentManager>,
    db: Arc<Database>,
    ocr_trigger_signals: Arc<OcrTriggerSignals>,
) -> Option<Arc<InputRecorder>> {
    match InputRecorder::new(consent_manager, db, ocr_trigger_signals).await {
        Ok(recorder) => {
            println!("Input recorder initialized successfully");
            Some(Arc::new(recorder))
        }
        Err(error) => {
            eprintln!("Warning: Failed to initialize input recorder: {}", error);
            eprintln!("Input recording features will be unavailable");
            None
        }
    }
}
