use super::dictation_supervision::initialize_dictation_supervisor;
use super::state::{AppState, DesktopCaptureRuntime};
use crate::core::config::Config;
use crate::core::consent::ConsentManager;
use crate::core::context_timeline;
use crate::core::database::Database;
use crate::core::input_recorder::InputRecorder;
use crate::core::keyboard_recorder::KeyboardRecorder;
use crate::core::multimodal::MultimodalService;
use crate::core::multimodal::{
    persist_foreground_transcript, DictationHelper, DictationSupervisor, PipelineAction,
    SupervisorCommand,
};
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
    // Vault manager: stores the app handle for helper-event forwarding.
    // The helper itself launches lazily on first vault op (§1.6), so
    // dev builds without a signed helper bundle stay fully usable.
    #[cfg(target_os = "macos")]
    crate::core::vault_client::init(app.handle());
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
        // Reserve the future vault directory with owner-only permissions and
        // a Spotlight never-index marker. Creates no vault state; see
        // docs/security/credential-vault-security-architecture.md §7.
        if let Err(error) = crate::core::vault_dir::ensure_future_vault_dir(&data_dir) {
            eprintln!("Warning: could not prepare future vault directory: {error}");
        }
        let recordings_path = data_dir.join("recordings");
        let storage = Arc::new(
            RecordingStorage::new(recordings_path, db.clone())
                .await
                .expect("Failed to initialize recording storage"),
        );

        let discarded = storage.discard_leftover_ocr_frames();
        if discarded > 0 {
            println!("Discarded {discarded} leftover OCR screenshots");
        }
        let ocr_processor = if config.ocr_enabled && config.capture_channels.ocr {
            initialize_ocr_processor(&db, &config).await
        } else {
            println!("OCR processor disabled by configuration");
            None
        };
        let ocr_trigger_signals = Arc::new(OcrTriggerSignals::new());
        let ocr_requested = config.ocr_enabled && config.capture_channels.ocr;
        // The frame loop feeds OCR screenshots (deleted after reading).
        // Video evidence stays disabled, so the recorder starts for OCR
        // even when screen_frames is off.
        let screen_recorder = if config.capture_channels.screen_frames || ocr_requested {
            initialize_screen_recorder(
                consent_manager.clone(),
                storage.clone(),
                ocr_trigger_signals.clone(),
                ocr_processor.clone(),
            )
            .await
        } else {
            println!("Screen recorder disabled by configuration");
            None
        };
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
        let dictation_commands = Arc::new(tokio::sync::Mutex::new(None));
        initialize_dictation_supervisor(
            shared_config.clone(),
            db.clone(),
            session_manager.clone(),
            app.handle().clone(),
            dictation_commands.clone(),
        );
        let (mobile_enabled, mobile_port) = shared_config
            .lock()
            .map(|config| (config.mobile_enabled, config.mobile_port))
            .unwrap_or((true, 8787));
        let agent_services =
            crate::app::agent_bridge_events::start_agent_bridge(app.handle(), shared_config.clone());
        crate::core::mobile::spawn_mobile_server(
            db.clone(),
            dictation_commands.clone(),
            session_manager.clone(),
            Some(app.handle().clone()),
            Some(agent_services),
            mobile_enabled,
            mobile_port,
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
            dictation_commands,
        });
    });

    // Regular app: Dock icon and Cmd+Tab entry. The tray icon stays,
    // but an accessory policy would hide SOURCE from the app switcher.
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    }

    super::tray::build_tray(app)?;

    Ok(())
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
