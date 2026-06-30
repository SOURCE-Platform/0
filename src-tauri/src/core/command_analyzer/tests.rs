#[cfg(test)]
mod tests {
    use super::super::{format_shortcut, CommandAnalyzer, CommandDatabase, CommandType};
    use crate::models::input::{
        AppContext, KeyEventType, KeyboardEvent, KeyboardShortcut, ModifierState,
    };

    #[test]
    fn test_command_database_lookup() {
        let database = CommandDatabase::new();

        #[cfg(target_os = "macos")]
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

        #[cfg(not(target_os = "macos"))]
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

        let definition = database.lookup(&shortcut).unwrap();
        assert_eq!(definition.name, "Copy");
    }

    #[test]
    fn test_format_shortcut() {
        #[cfg(target_os = "macos")]
        assert_eq!(
            format_shortcut(
                &ModifierState {
                    meta: true,
                    shift: true,
                    ctrl: false,
                    alt: false
                },
                "Z"
            ),
            "⇧⌘Z"
        );

        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            format_shortcut(
                &ModifierState {
                    meta: false,
                    shift: true,
                    ctrl: true,
                    alt: false
                },
                "Z"
            ),
            "Ctrl+Shift+Z"
        );
    }

    #[test]
    fn test_detect_command() {
        let analyzer = CommandAnalyzer::new();
        let event = KeyboardEvent {
            timestamp: chrono::Utc::now().timestamp_millis(),
            event_type: KeyEventType::KeyDown,
            key_code: 8,
            key_char: Some('c'),
            modifiers: ModifierState {
                meta: cfg!(target_os = "macos"),
                shift: false,
                ctrl: !cfg!(target_os = "macos"),
                alt: false,
            },
            app_context: AppContext::new("TestApp".to_string(), "Test".to_string(), 1234),
            ui_element: None,
            is_sensitive: false,
        };

        let mut analyzer = analyzer;
        let commands = analyzer.analyze_events(vec![event]);
        assert_eq!(commands.len(), 1);
        assert!(matches!(commands[0].command_type, CommandType::System));
    }
}
