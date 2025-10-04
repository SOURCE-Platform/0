// Command and keyboard shortcut recognition

use crate::core::database::Database;
use crate::models::input::{KeyEventType, KeyboardEvent, KeyboardShortcut, ModifierState};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

// ==============================================================================
// Command Types
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub id: Uuid,
    pub timestamp: i64,
    pub shortcut: KeyboardShortcut,
    pub command_type: CommandType,
    pub app_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandType {
    System,              // OS-level shortcuts
    ApplicationSpecific, // App-specific shortcuts
    Custom,              // User-defined shortcuts
    Unknown,
}

impl CommandType {
    pub fn to_string(&self) -> &'static str {
        match self {
            CommandType::System => "system",
            CommandType::ApplicationSpecific => "application_specific",
            CommandType::Custom => "custom",
            CommandType::Unknown => "unknown",
        }
    }
}

// ==============================================================================
// Command Database
// ==============================================================================

#[derive(Debug, Clone)]
pub struct CommandDefinition {
    pub shortcut: KeyboardShortcut,
    pub name: String,
    pub description: String,
    pub command_type: CommandType,
    pub platforms: Vec<String>, // ["macos", "windows", "linux"]
    pub applications: Option<Vec<String>>, // Specific apps or None for global
}

pub struct CommandDatabase {
    shortcuts: HashMap<String, CommandDefinition>,
}

impl CommandDatabase {
    pub fn new() -> Self {
        let mut shortcuts = HashMap::new();

        // Get current platform
        #[cfg(target_os = "macos")]
        let _platform = "macos";
        #[cfg(target_os = "windows")]
        let _platform = "windows";
        #[cfg(target_os = "linux")]
        let _platform = "linux";

        // macOS System Shortcuts
        #[cfg(target_os = "macos")]
        {
            // Cmd+C - Copy
            shortcuts.insert(
                "cmd+c".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "C".to_string(),
                        display: "⌘C".to_string(),
                    },
                    name: "Copy".to_string(),
                    description: "Copy selected content".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+V - Paste
            shortcuts.insert(
                "cmd+v".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "V".to_string(),
                        display: "⌘V".to_string(),
                    },
                    name: "Paste".to_string(),
                    description: "Paste from clipboard".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+X - Cut
            shortcuts.insert(
                "cmd+x".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "X".to_string(),
                        display: "⌘X".to_string(),
                    },
                    name: "Cut".to_string(),
                    description: "Cut selected content".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+S - Save
            shortcuts.insert(
                "cmd+s".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "S".to_string(),
                        display: "⌘S".to_string(),
                    },
                    name: "Save".to_string(),
                    description: "Save current document".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+Z - Undo
            shortcuts.insert(
                "cmd+z".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "Z".to_string(),
                        display: "⌘Z".to_string(),
                    },
                    name: "Undo".to_string(),
                    description: "Undo last action".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+Shift+Z - Redo
            shortcuts.insert(
                "cmd+shift+z".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: true,
                            ctrl: false,
                            alt: false,
                        },
                        key: "Z".to_string(),
                        display: "⌘⇧Z".to_string(),
                    },
                    name: "Redo".to_string(),
                    description: "Redo last action".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+A - Select All
            shortcuts.insert(
                "cmd+a".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "A".to_string(),
                        display: "⌘A".to_string(),
                    },
                    name: "Select All".to_string(),
                    description: "Select all content".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+F - Find
            shortcuts.insert(
                "cmd+f".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "F".to_string(),
                        display: "⌘F".to_string(),
                    },
                    name: "Find".to_string(),
                    description: "Open find dialog".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+N - New
            shortcuts.insert(
                "cmd+n".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "N".to_string(),
                        display: "⌘N".to_string(),
                    },
                    name: "New".to_string(),
                    description: "Create new document/window".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+W - Close Window
            shortcuts.insert(
                "cmd+w".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "W".to_string(),
                        display: "⌘W".to_string(),
                    },
                    name: "Close Window".to_string(),
                    description: "Close current window".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+Q - Quit
            shortcuts.insert(
                "cmd+q".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "Q".to_string(),
                        display: "⌘Q".to_string(),
                    },
                    name: "Quit".to_string(),
                    description: "Quit application".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );

            // Cmd+T - New Tab
            shortcuts.insert(
                "cmd+t".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: true,
                            shift: false,
                            ctrl: false,
                            alt: false,
                        },
                        key: "T".to_string(),
                        display: "⌘T".to_string(),
                    },
                    name: "New Tab".to_string(),
                    description: "Open new tab".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["macos".to_string()],
                    applications: None,
                },
            );
        }

        // Windows/Linux System Shortcuts
        #[cfg(not(target_os = "macos"))]
        {
            // Ctrl+C - Copy
            shortcuts.insert(
                "ctrl+c".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "C".to_string(),
                        display: "Ctrl+C".to_string(),
                    },
                    name: "Copy".to_string(),
                    description: "Copy selected content".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+V - Paste
            shortcuts.insert(
                "ctrl+v".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "V".to_string(),
                        display: "Ctrl+V".to_string(),
                    },
                    name: "Paste".to_string(),
                    description: "Paste from clipboard".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+X - Cut
            shortcuts.insert(
                "ctrl+x".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "X".to_string(),
                        display: "Ctrl+X".to_string(),
                    },
                    name: "Cut".to_string(),
                    description: "Cut selected content".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+S - Save
            shortcuts.insert(
                "ctrl+s".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "S".to_string(),
                        display: "Ctrl+S".to_string(),
                    },
                    name: "Save".to_string(),
                    description: "Save current document".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+Z - Undo
            shortcuts.insert(
                "ctrl+z".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "Z".to_string(),
                        display: "Ctrl+Z".to_string(),
                    },
                    name: "Undo".to_string(),
                    description: "Undo last action".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+Y - Redo (Windows/Linux use Ctrl+Y instead of Ctrl+Shift+Z)
            shortcuts.insert(
                "ctrl+y".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "Y".to_string(),
                        display: "Ctrl+Y".to_string(),
                    },
                    name: "Redo".to_string(),
                    description: "Redo last action".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+A - Select All
            shortcuts.insert(
                "ctrl+a".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "A".to_string(),
                        display: "Ctrl+A".to_string(),
                    },
                    name: "Select All".to_string(),
                    description: "Select all content".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+F - Find
            shortcuts.insert(
                "ctrl+f".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "F".to_string(),
                        display: "Ctrl+F".to_string(),
                    },
                    name: "Find".to_string(),
                    description: "Open find dialog".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+N - New
            shortcuts.insert(
                "ctrl+n".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "N".to_string(),
                        display: "Ctrl+N".to_string(),
                    },
                    name: "New".to_string(),
                    description: "Create new document/window".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+W - Close Window
            shortcuts.insert(
                "ctrl+w".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "W".to_string(),
                        display: "Ctrl+W".to_string(),
                    },
                    name: "Close Window".to_string(),
                    description: "Close current window".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Ctrl+T - New Tab
            shortcuts.insert(
                "ctrl+t".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: true,
                            alt: false,
                        },
                        key: "T".to_string(),
                        display: "Ctrl+T".to_string(),
                    },
                    name: "New Tab".to_string(),
                    description: "Open new tab".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string(), "linux".to_string()],
                    applications: None,
                },
            );

            // Alt+F4 - Close (Windows)
            #[cfg(target_os = "windows")]
            shortcuts.insert(
                "alt+f4".to_string(),
                CommandDefinition {
                    shortcut: KeyboardShortcut {
                        modifiers: ModifierState {
                            meta: false,
                            shift: false,
                            ctrl: false,
                            alt: true,
                        },
                        key: "F4".to_string(),
                        display: "Alt+F4".to_string(),
                    },
                    name: "Close".to_string(),
                    description: "Close application".to_string(),
                    command_type: CommandType::System,
                    platforms: vec!["windows".to_string()],
                    applications: None,
                },
            );
        }

        Self { shortcuts }
    }

    pub fn lookup(&self, shortcut: &KeyboardShortcut) -> Option<&CommandDefinition> {
        let key = self.shortcut_to_key(shortcut);
        self.shortcuts.get(&key)
    }

    fn shortcut_to_key(&self, shortcut: &KeyboardShortcut) -> String {
        let mut parts = Vec::new();

        if shortcut.modifiers.ctrl {
            parts.push("ctrl");
        }
        if shortcut.modifiers.shift {
            parts.push("shift");
        }
        if shortcut.modifiers.alt {
            parts.push("alt");
        }
        if shortcut.modifiers.meta {
            parts.push("cmd");
        }

        let key_lower = shortcut.key.to_lowercase();
        parts.push(&key_lower);

        parts.join("+")
    }
}

// ==============================================================================
// Command Analyzer
// ==============================================================================

pub struct CommandAnalyzer {
    command_database: CommandDatabase,
    keyboard_buffer: VecDeque<KeyboardEvent>,
    buffer_duration: Duration,
}

impl CommandAnalyzer {
    pub fn new() -> Self {
        Self {
            command_database: CommandDatabase::new(),
            keyboard_buffer: VecDeque::new(),
            buffer_duration: Duration::from_millis(500),
        }
    }

    pub fn analyze_events(&mut self, events: Vec<KeyboardEvent>) -> Vec<Command> {
        let mut commands = Vec::new();

        for event in events {
            // Only process KeyDown events
            if !matches!(event.event_type, KeyEventType::KeyDown) {
                continue;
            }

            // Add to buffer
            self.keyboard_buffer.push_back(event.clone());

            // Remove old events from buffer
            self.clean_buffer();

            // Check if this forms a command
            if let Some(command) = self.detect_command(&event) {
                commands.push(command);
            }
        }

        commands
    }

    fn clean_buffer(&mut self) {
        let now = chrono::Utc::now().timestamp_millis();

        while let Some(front) = self.keyboard_buffer.front() {
            if now - front.timestamp > self.buffer_duration.as_millis() as i64 {
                self.keyboard_buffer.pop_front();
            } else {
                break;
            }
        }
    }

    fn detect_command(&self, event: &KeyboardEvent) -> Option<Command> {
        // Check if event has modifiers (likely a command)
        if !self.has_any_modifier(&event.modifiers) {
            return None;
        }

        // Skip sensitive input
        if event.is_sensitive {
            return None;
        }

        let key = event.key_char?.to_string().to_uppercase();

        let shortcut = KeyboardShortcut {
            modifiers: event.modifiers.clone(),
            key: key.clone(),
            display: self.format_shortcut(&event.modifiers, &key),
        };

        // Lookup in command database
        if let Some(definition) = self.command_database.lookup(&shortcut) {
            return Some(Command {
                id: Uuid::new_v4(),
                timestamp: event.timestamp,
                shortcut,
                command_type: definition.command_type.clone(),
                app_name: event.app_context.app_name.clone(),
                description: definition.description.clone(),
            });
        }

        // Unknown command but has modifiers
        Some(Command {
            id: Uuid::new_v4(),
            timestamp: event.timestamp,
            shortcut,
            command_type: CommandType::Unknown,
            app_name: event.app_context.app_name.clone(),
            description: "Unknown shortcut".to_string(),
        })
    }

    fn has_any_modifier(&self, modifiers: &ModifierState) -> bool {
        modifiers.ctrl || modifiers.alt || modifiers.shift || modifiers.meta
    }

    fn format_shortcut(&self, modifiers: &ModifierState, key: &str) -> String {
        let mut parts = Vec::new();

        #[cfg(target_os = "macos")]
        {
            if modifiers.ctrl {
                parts.push("⌃");
            }
            if modifiers.alt {
                parts.push("⌥");
            }
            if modifiers.shift {
                parts.push("⇧");
            }
            if modifiers.meta {
                parts.push("⌘");
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            if modifiers.ctrl {
                parts.push("Ctrl");
            }
            if modifiers.alt {
                parts.push("Alt");
            }
            if modifiers.shift {
                parts.push("Shift");
            }
            if modifiers.meta {
                parts.push("Win");
            }
        }

        parts.push(key);

        #[cfg(target_os = "macos")]
        {
            parts.join("")
        }
        #[cfg(not(target_os = "macos"))]
        {
            parts.join("+")
        }
    }

    /// Get command statistics from database
    pub async fn get_command_stats(
        db: &Arc<Database>,
        session_id: Option<Uuid>,
    ) -> Result<CommandStats, Box<dyn std::error::Error + Send + Sync>> {
        let pool = db.pool();

        let rows: Vec<CommandStatsRow> = if let Some(sid) = session_id {
            sqlx::query_as(
                r#"
                SELECT shortcut, app_name, COUNT(*) as count
                FROM commands
                WHERE session_id = ?
                GROUP BY shortcut, app_name
                ORDER BY count DESC
                "#,
            )
            .bind(sid.to_string())
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT shortcut, app_name, COUNT(*) as count
                FROM commands
                GROUP BY shortcut, app_name
                ORDER BY count DESC
                "#,
            )
            .fetch_all(pool)
            .await?
        };

        let mut most_used: HashMap<String, u32> = HashMap::new();
        let mut by_app: HashMap<String, Vec<(String, u32)>> = HashMap::new();

        for row in rows {
            *most_used.entry(row.shortcut.clone()).or_insert(0) += row.count as u32;
            by_app
                .entry(row.app_name.clone())
                .or_insert_with(Vec::new)
                .push((row.shortcut, row.count as u32));
        }

        let mut most_used_shortcuts: Vec<(String, u32)> = most_used.into_iter().collect();
        most_used_shortcuts.sort_by(|a, b| b.1.cmp(&a.1));

        Ok(CommandStats {
            total_shortcuts: most_used_shortcuts.iter().map(|(_, c)| c).sum(),
            unique_shortcuts: most_used_shortcuts.len() as u32,
            most_used_shortcuts: most_used_shortcuts.into_iter().take(20).collect(),
            shortcuts_by_app: by_app,
        })
    }

    /// Store a command in the database
    pub async fn store_command(
        db: &Arc<Database>,
        session_id: Uuid,
        command: Command,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = db.pool();

        sqlx::query(
            r#"
            INSERT INTO commands (id, session_id, timestamp, shortcut, command_type, app_name, description)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(command.id.to_string())
        .bind(session_id.to_string())
        .bind(command.timestamp)
        .bind(command.shortcut.display)
        .bind(command.command_type.to_string())
        .bind(command.app_name)
        .bind(command.description)
        .execute(pool)
        .await?;

        Ok(())
    }
}

// ==============================================================================
// Command Statistics
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStats {
    pub most_used_shortcuts: Vec<(String, u32)>,
    pub shortcuts_by_app: HashMap<String, Vec<(String, u32)>>,
    pub total_shortcuts: u32,
    pub unique_shortcuts: u32,
}

#[derive(Debug, sqlx::FromRow)]
struct CommandStatsRow {
    shortcut: String,
    app_name: String,
    count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::input::{AppContext, KeyEventType};

    #[test]
    fn test_command_database_lookup() {
        let db = CommandDatabase::new();

        #[cfg(target_os = "macos")]
        {
            let shortcut = KeyboardShortcut {
                modifiers: ModifierState {
                    meta: true,
                    shift: false,
                    ctrl: false,
                    alt: false,
                },
                key: "C".to_string(),
                display: "⌘C".to_string(),
            };

            let definition = db.lookup(&shortcut);
            assert!(definition.is_some());
            assert_eq!(definition.unwrap().name, "Copy");
        }

        #[cfg(not(target_os = "macos"))]
        {
            let shortcut = KeyboardShortcut {
                modifiers: ModifierState {
                    meta: false,
                    shift: false,
                    ctrl: true,
                    alt: false,
                },
                key: "C".to_string(),
                display: "Ctrl+C".to_string(),
            };

            let definition = db.lookup(&shortcut);
            assert!(definition.is_some());
            assert_eq!(definition.unwrap().name, "Copy");
        }
    }

    #[test]
    fn test_format_shortcut() {
        let analyzer = CommandAnalyzer::new();

        #[cfg(target_os = "macos")]
        {
            let modifiers = ModifierState {
                meta: true,
                shift: true,
                ctrl: false,
                alt: false,
            };
            let display = analyzer.format_shortcut(&modifiers, "Z");
            assert_eq!(display, "⇧⌘Z");
        }

        #[cfg(not(target_os = "macos"))]
        {
            let modifiers = ModifierState {
                meta: false,
                shift: true,
                ctrl: true,
                alt: false,
            };
            let display = analyzer.format_shortcut(&modifiers, "Z");
            assert_eq!(display, "Ctrl+Shift+Z");
        }
    }

    #[test]
    fn test_detect_command() {
        let analyzer = CommandAnalyzer::new();

        #[cfg(target_os = "macos")]
        {
            let event = KeyboardEvent {
                timestamp: chrono::Utc::now().timestamp_millis(),
                event_type: KeyEventType::KeyDown,
                key_code: 8, // C key
                key_char: Some('c'),
                modifiers: ModifierState {
                    meta: true,
                    shift: false,
                    ctrl: false,
                    alt: false,
                },
                app_context: AppContext::new("TestApp".to_string(), "Test".to_string(), 1234),
                ui_element: None,
                is_sensitive: false,
            };

            let command = analyzer.detect_command(&event);
            assert!(command.is_some());
            let cmd = command.unwrap();
            assert_eq!(cmd.description, "Copy selected content");
            assert!(matches!(cmd.command_type, CommandType::System));
        }
    }
}
