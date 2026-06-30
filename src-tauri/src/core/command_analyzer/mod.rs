use crate::core::database::Database;
use crate::models::input::{KeyEventType, KeyboardEvent, KeyboardShortcut, ModifierState};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

mod database_defs;
mod tests;
mod types;

use types::CommandStatsRow;
pub use types::{Command, CommandDefinition, CommandStats, CommandType};

pub struct CommandDatabase {
    shortcuts: HashMap<String, CommandDefinition>,
}

pub struct CommandAnalyzer {
    command_database: CommandDatabase,
    keyboard_buffer: VecDeque<KeyboardEvent>,
    buffer_duration: Duration,
}

impl CommandDatabase {
    pub fn new() -> Self {
        Self {
            shortcuts: database_defs::build_shortcuts(),
        }
    }

    pub fn lookup(&self, shortcut: &KeyboardShortcut) -> Option<&CommandDefinition> {
        let key = shortcut_to_key(shortcut);
        self.shortcuts.get(&key)
    }
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
            if !matches!(event.event_type, KeyEventType::KeyDown) {
                continue;
            }
            self.keyboard_buffer.push_back(event.clone());
            self.clean_buffer();
            if let Some(command) = self.detect_command(&event) {
                commands.push(command);
            }
        }
        commands
    }

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

        let mut most_used = HashMap::new();
        let mut by_app = HashMap::new();
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
            total_shortcuts: most_used_shortcuts.iter().map(|(_, count)| count).sum(),
            unique_shortcuts: most_used_shortcuts.len() as u32,
            most_used_shortcuts: most_used_shortcuts.into_iter().take(20).collect(),
            shortcuts_by_app: by_app,
        })
    }

    pub async fn store_command(
        db: &Arc<Database>,
        session_id: Uuid,
        command: Command,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        .execute(db.pool())
        .await?;
        Ok(())
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
        if !has_any_modifier(&event.modifiers) || event.is_sensitive {
            return None;
        }

        let key = event.key_char?.to_string().to_uppercase();
        let shortcut = KeyboardShortcut {
            modifiers: event.modifiers.clone(),
            key: key.clone(),
            display: format_shortcut(&event.modifiers, &key),
        };

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

        Some(Command {
            id: Uuid::new_v4(),
            timestamp: event.timestamp,
            shortcut,
            command_type: CommandType::Unknown,
            app_name: event.app_context.app_name.clone(),
            description: "Unknown shortcut".to_string(),
        })
    }
}

fn shortcut_to_key(shortcut: &KeyboardShortcut) -> String {
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

fn has_any_modifier(modifiers: &ModifierState) -> bool {
    modifiers.ctrl || modifiers.alt || modifiers.shift || modifiers.meta
}

pub(crate) fn format_shortcut(modifiers: &ModifierState, key: &str) -> String {
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
        parts.push(key);
        parts.join("")
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
        parts.push(key);
        parts.join("+")
    }
}
