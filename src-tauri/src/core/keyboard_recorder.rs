use crate::core::consent::ConsentManager;
use crate::core::database::Database;
use crate::models::input::{KeyboardEvent, KeyEventType, KeyboardStats};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// Platform-specific keyboard listener
#[cfg(target_os = "macos")]
use crate::platform::input::MacOSKeyboardListener as PlatformKeyboardListener;
#[cfg(target_os = "windows")]
use crate::platform::input::WindowsKeyboardListener as PlatformKeyboardListener;
#[cfg(target_os = "linux")]
use crate::platform::input::LinuxKeyboardListener as PlatformKeyboardListener;

// ==============================================================================
// Database Models
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct KeyboardEventRecord {
    pub id: String,
    pub session_id: String,
    pub timestamp: i64,
    pub event_type: String,
    pub key_code: i64,
    pub key_char: Option<String>,
    pub modifiers: String, // JSON string of ModifierState
    pub app_name: String,
    pub window_title: String,
    pub process_id: i64,
    pub is_sensitive: i64, // SQLite boolean (0 or 1)
}

// ==============================================================================
// Keyboard Recorder
// ==============================================================================

pub struct KeyboardRecorder {
    db: Arc<Database>,
    consent_manager: Arc<ConsentManager>,
    listener: Arc<RwLock<Option<PlatformKeyboardListener>>>,
    current_session_id: Arc<RwLock<Option<String>>>,
    is_recording: Arc<RwLock<bool>>,
}

impl KeyboardRecorder {
    pub async fn new(
        consent_manager: Arc<ConsentManager>,
        db: Arc<Database>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Initialize database schema
        Self::init_schema(&db).await?;

        Ok(Self {
            db,
            consent_manager,
            listener: Arc::new(RwLock::new(None)),
            current_session_id: Arc::new(RwLock::new(None)),
            is_recording: Arc::new(RwLock::new(false)),
        })
    }

    async fn init_schema(db: &Arc<Database>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = db.pool();

        // Create keyboard_events table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS keyboard_events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                key_code INTEGER NOT NULL,
                key_char TEXT,
                modifiers TEXT NOT NULL,
                app_name TEXT NOT NULL,
                window_title TEXT NOT NULL,
                process_id INTEGER NOT NULL,
                is_sensitive INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            )"
        )
        .execute(pool)
        .await?;

        // Create indexes for efficient queries
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_keyboard_events_session ON keyboard_events(session_id)")
            .execute(pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_keyboard_events_timestamp ON keyboard_events(timestamp)")
            .execute(pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_keyboard_events_app ON keyboard_events(app_name)")
            .execute(pool)
            .await?;

        Ok(())
    }

    pub async fn start_recording(&self, session_id: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check if already recording
        let mut is_recording = self.is_recording.write().await;
        if *is_recording {
            return Err("Already recording keyboard events".into());
        }

        // Store session ID
        *self.current_session_id.write().await = Some(session_id.clone());

        // Create and start listener
        let (listener, mut event_rx) = PlatformKeyboardListener::new(self.consent_manager.clone())?;

        listener.start_listening().await?;

        *self.listener.write().await = Some(listener);
        *is_recording = true;

        // Spawn background task to process events
        let db = self.db.clone();
        let current_session_id = self.current_session_id.clone();
        let is_recording_clone = self.is_recording.clone();

        tokio::spawn(async move {
            Self::process_events(event_rx, db, current_session_id, is_recording_clone).await;
        });

        println!("Started keyboard recording for session {}", session_id);
        Ok(())
    }

    pub async fn stop_recording(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut is_recording = self.is_recording.write().await;
        if !*is_recording {
            return Ok(());
        }

        // Stop listener
        if let Some(listener) = self.listener.write().await.take() {
            listener.stop_listening().await?;
        }

        *is_recording = false;
        *self.current_session_id.write().await = None;

        println!("Stopped keyboard recording");
        Ok(())
    }

    async fn process_events(
        mut event_rx: mpsc::UnboundedReceiver<KeyboardEvent>,
        db: Arc<Database>,
        current_session_id: Arc<RwLock<Option<String>>>,
        is_recording: Arc<RwLock<bool>>,
    ) {
        while let Some(event) = event_rx.recv().await {
            // Check if still recording
            if !*is_recording.read().await {
                break;
            }

            let session_id = match &*current_session_id.read().await {
                Some(id) => id.clone(),
                None => continue,
            };

            // Store event in database (respecting privacy - sensitive events already filtered)
            if let Err(e) = Self::store_event(&db, &session_id, &event).await {
                eprintln!("Error storing keyboard event: {}", e);
            }
        }
    }

    async fn store_event(
        db: &Arc<Database>,
        session_id: &str,
        event: &KeyboardEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let id = uuid::Uuid::new_v4().to_string();

        // Serialize modifiers to JSON
        let modifiers_json = serde_json::to_string(&event.modifiers)?;

        sqlx::query(
            "INSERT INTO keyboard_events
             (id, session_id, timestamp, event_type, key_code, key_char, modifiers,
              app_name, window_title, process_id, is_sensitive)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(session_id)
        .bind(event.timestamp)
        .bind(event.event_type.to_string())
        .bind(event.key_code as i64)
        .bind(event.key_char.map(|c| c.to_string()))
        .bind(modifiers_json)
        .bind(&event.app_context.app_name)
        .bind(&event.app_context.window_title)
        .bind(event.app_context.process_id as i64)
        .bind(if event.is_sensitive { 1 } else { 0 })
        .execute(db.pool())
        .await?;

        Ok(())
    }

    pub async fn get_keyboard_stats(&self, session_id: String) -> Result<KeyboardStats, Box<dyn std::error::Error + Send + Sync>> {
        // Get all keyboard events for this session
        let events = sqlx::query_as::<_, KeyboardEventRecord>(
            "SELECT * FROM keyboard_events WHERE session_id = ? ORDER BY timestamp ASC"
        )
        .bind(&session_id)
        .fetch_all(self.db.pool())
        .await?;

        if events.is_empty() {
            return Ok(KeyboardStats {
                session_id,
                total_keystrokes: 0,
                keys_per_minute: 0.0,
                most_used_keys: Vec::new(),
                shortcut_usage: Vec::new(),
                typing_speed_wpm: None,
            });
        }

        // Calculate total keystrokes (only KeyDown events)
        let total_keystrokes = events.iter()
            .filter(|e| e.event_type == "key_down")
            .count() as u64;

        // Calculate duration
        let first_timestamp = events.first().unwrap().timestamp;
        let last_timestamp = events.last().unwrap().timestamp;
        let duration_minutes = ((last_timestamp - first_timestamp) as f32 / 60000.0).max(1.0);

        // Calculate keys per minute
        let keys_per_minute = total_keystrokes as f32 / duration_minutes;

        // Count most used keys
        let mut key_counts: HashMap<char, u32> = HashMap::new();
        for event in &events {
            if event.event_type == "key_down" {
                if let Some(ref key_str) = event.key_char {
                    if let Some(key_char) = key_str.chars().next() {
                        *key_counts.entry(key_char).or_insert(0) += 1;
                    }
                }
            }
        }

        let mut most_used_keys: Vec<(char, u32)> = key_counts.into_iter().collect();
        most_used_keys.sort_by(|a, b| b.1.cmp(&a.1));
        most_used_keys.truncate(10); // Top 10

        // Detect common shortcuts
        let mut shortcut_counts: HashMap<String, u32> = HashMap::new();
        for event in &events {
            if event.event_type == "key_down" {
                if let Ok(modifiers) = serde_json::from_str::<crate::models::input::ModifierState>(&event.modifiers) {
                    if !modifiers.is_empty() {
                        if let Some(ref key_str) = event.key_char {
                            let shortcut = format!("{}+{}", modifiers.to_string(), key_str);
                            *shortcut_counts.entry(shortcut).or_insert(0) += 1;
                        }
                    }
                }
            }
        }

        let mut shortcut_usage: Vec<(String, u32)> = shortcut_counts.into_iter().collect();
        shortcut_usage.sort_by(|a, b| b.1.cmp(&a.1));
        shortcut_usage.truncate(10); // Top 10

        Ok(KeyboardStats {
            session_id,
            total_keystrokes,
            keys_per_minute,
            most_used_keys,
            shortcut_usage,
            typing_speed_wpm: None, // Would require word boundary detection
        })
    }

    pub async fn is_recording(&self) -> bool {
        *self.is_recording.read().await
    }
}
