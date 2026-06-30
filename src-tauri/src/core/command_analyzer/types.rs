use crate::models::input::KeyboardShortcut;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

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
    System,
    ApplicationSpecific,
    Custom,
    Unknown,
}

impl CommandType {
    pub fn to_string(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ApplicationSpecific => "application_specific",
            Self::Custom => "custom",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandDefinition {
    pub shortcut: KeyboardShortcut,
    pub name: String,
    pub description: String,
    pub command_type: CommandType,
    pub platforms: Vec<String>,
    pub applications: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStats {
    pub most_used_shortcuts: Vec<(String, u32)>,
    pub shortcuts_by_app: HashMap<String, Vec<(String, u32)>>,
    pub total_shortcuts: u32,
    pub unique_shortcuts: u32,
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct CommandStatsRow {
    pub shortcut: String,
    pub app_name: String,
    pub count: i64,
}
