use super::types::{CommandDefinition, CommandType};
use crate::models::input::{KeyboardShortcut, ModifierState};
use std::collections::HashMap;

pub(super) fn build_shortcuts() -> HashMap<String, CommandDefinition> {
    let mut shortcuts = HashMap::new();
    add_common_shortcuts(&mut shortcuts);
    add_platform_shortcuts(&mut shortcuts);
    shortcuts
}

fn add_common_shortcuts(shortcuts: &mut HashMap<String, CommandDefinition>) {
    #[cfg(target_os = "macos")]
    let modifier = ModifierState {
        meta: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    #[cfg(not(target_os = "macos"))]
    let modifier = ModifierState {
        meta: false,
        shift: false,
        ctrl: true,
        alt: false,
    };

    let display_prefix = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    };
    let platform = if cfg!(target_os = "macos") {
        vec!["macos".to_string()]
    } else {
        vec!["windows".to_string(), "linux".to_string()]
    };

    for (key, name, description) in [
        ("C", "Copy", "Copy selected content"),
        ("V", "Paste", "Paste from clipboard"),
        ("X", "Cut", "Cut selected content"),
        ("S", "Save", "Save current document"),
        ("Z", "Undo", "Undo last action"),
        ("A", "Select All", "Select all content"),
        ("F", "Find", "Open find dialog"),
        ("N", "New", "Create new document/window"),
        ("W", "Close Window", "Close current window"),
        ("T", "New Tab", "Open new tab"),
    ] {
        shortcuts.insert(
            shortcut_key(&modifier, key),
            definition(
                modifier.clone(),
                key,
                &format!("{}{}", display_prefix, key),
                name,
                description,
                platform.clone(),
            ),
        );
    }

    #[cfg(target_os = "macos")]
    shortcuts.insert(
        shortcut_key(
            &ModifierState {
                shift: true,
                ..modifier
            },
            "Z",
        ),
        definition(
            ModifierState {
                shift: true,
                ..modifier
            },
            "Z",
            "⌘⇧Z",
            "Redo",
            "Redo last action",
            vec!["macos".to_string()],
        ),
    );

    #[cfg(not(target_os = "macos"))]
    shortcuts.insert(
        shortcut_key(&modifier, "Y"),
        definition(
            modifier,
            "Y",
            "Ctrl+Y",
            "Redo",
            "Redo last action",
            vec!["windows".to_string(), "linux".to_string()],
        ),
    );
}

fn add_platform_shortcuts(shortcuts: &mut HashMap<String, CommandDefinition>) {
    #[cfg(target_os = "macos")]
    {
        let cmd = ModifierState {
            meta: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        shortcuts.insert(
            shortcut_key(&cmd, "Q"),
            definition(
                cmd,
                "Q",
                "⌘Q",
                "Quit",
                "Quit application",
                vec!["macos".to_string()],
            ),
        );
    }

    #[cfg(target_os = "windows")]
    {
        let alt = ModifierState {
            meta: false,
            shift: false,
            ctrl: false,
            alt: true,
        };
        shortcuts.insert(
            shortcut_key(&alt, "F4"),
            definition(
                alt,
                "F4",
                "Alt+F4",
                "Close",
                "Close application",
                vec!["windows".to_string()],
            ),
        );
    }
}

fn definition(
    modifiers: ModifierState,
    key: &str,
    display: &str,
    name: &str,
    description: &str,
    platforms: Vec<String>,
) -> CommandDefinition {
    CommandDefinition {
        shortcut: KeyboardShortcut {
            modifiers,
            key: key.to_string(),
            display: display.to_string(),
        },
        name: name.to_string(),
        description: description.to_string(),
        command_type: CommandType::System,
        platforms,
        applications: None,
    }
}

fn shortcut_key(modifiers: &ModifierState, key: &str) -> String {
    let mut parts = Vec::new();
    if modifiers.ctrl {
        parts.push("ctrl");
    }
    if modifiers.shift {
        parts.push("shift");
    }
    if modifiers.alt {
        parts.push("alt");
    }
    if modifiers.meta {
        parts.push("cmd");
    }
    let lower = key.to_lowercase();
    parts.push(&lower);
    parts.join("+")
}
