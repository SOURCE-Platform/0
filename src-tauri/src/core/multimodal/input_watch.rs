//! Decide which microphone is actually in use as devices come and go.
//!
//! The microphone chosen in Settings is a preference, not a guarantee: an
//! adapter gets unplugged, a dock gets connected. This works out which device
//! recording will really land on, so the app can follow the change and say so
//! instead of quietly recording from a microphone nobody is speaking into.

use cpal::traits::{DeviceTrait, HostTrait};

const NAME_PREFIX: &str = "microphone-name:";

/// The input devices macOS is offering right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputSnapshot {
    pub default_name: Option<String>,
    pub devices: Vec<String>,
}

/// Which microphone recording will actually use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveInput {
    pub name: Option<String>,
    /// True when the chosen microphone is not plugged in and this is a fallback.
    pub falling_back: bool,
}

/// The device name inside a stored `selected_audio_input_id`, if one is pinned.
pub fn pinned_name(selected_audio_input_id: Option<&str>) -> Option<String> {
    let value = selected_audio_input_id?.strip_prefix(NAME_PREFIX)?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Resolve the chosen microphone against the devices that exist.
pub fn resolve_active(pinned: Option<&str>, snapshot: &InputSnapshot) -> ActiveInput {
    match pinned {
        Some(name) if snapshot.devices.iter().any(|device| device == name) => ActiveInput {
            name: Some(name.to_string()),
            falling_back: false,
        },
        Some(_) => ActiveInput {
            name: snapshot.default_name.clone(),
            falling_back: snapshot.default_name.is_some(),
        },
        None => ActiveInput {
            name: snapshot.default_name.clone(),
            falling_back: false,
        },
    }
}

/// What to tell the user when the active microphone changes. `None` when the
/// change needs no announcement (nothing was recording from a device yet).
pub fn switch_message(previous: &ActiveInput, current: &ActiveInput, pinned: Option<&str>) -> Option<String> {
    if previous == current {
        return None;
    }
    let name = current.name.as_deref()?;
    if current.falling_back {
        let chosen = pinned.unwrap_or("The chosen microphone");
        return Some(format!("{chosen} is unplugged. Recording from {name}."));
    }
    match previous.name.as_deref() {
        Some(_) => Some(format!("Microphone switched to {name}.")),
        // First reading after launch: state it once rather than announce a switch.
        None => None,
    }
}

/// Ask Core Audio what input devices exist. Blocking: call it off the async runtime.
pub fn read_snapshot() -> InputSnapshot {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map(|list| list.filter_map(|device| device.name().ok()).collect())
        .unwrap_or_default();
    InputSnapshot {
        default_name: host.default_input_device().and_then(|device| device.name().ok()),
        devices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(default_name: &str, devices: &[&str]) -> InputSnapshot {
        InputSnapshot {
            default_name: Some(default_name.to_string()),
            devices: devices.iter().map(|d| d.to_string()).collect(),
        }
    }

    #[test]
    fn reads_the_pinned_device_name() {
        assert_eq!(pinned_name(Some("microphone-name:USB-C Adapter")).as_deref(), Some("USB-C Adapter"));
        assert_eq!(pinned_name(Some("desktop_output:system")), None);
        assert_eq!(pinned_name(None), None);
    }

    #[test]
    fn uses_the_pinned_microphone_when_it_is_plugged_in() {
        let snap = snapshot("MacBook Air Microphone", &["MacBook Air Microphone", "USB-C Adapter"]);
        let active = resolve_active(Some("USB-C Adapter"), &snap);
        assert_eq!(active.name.as_deref(), Some("USB-C Adapter"));
        assert!(!active.falling_back);
    }

    #[test]
    fn falls_back_to_the_system_default_when_the_pinned_one_is_gone() {
        let snap = snapshot("MacBook Air Microphone", &["MacBook Air Microphone"]);
        let active = resolve_active(Some("USB-C Adapter"), &snap);
        assert_eq!(active.name.as_deref(), Some("MacBook Air Microphone"));
        assert!(active.falling_back);
    }

    #[test]
    fn follows_the_system_default_when_nothing_is_pinned() {
        let snap = snapshot("USB-C Adapter", &["MacBook Air Microphone", "USB-C Adapter"]);
        assert_eq!(resolve_active(None, &snap).name.as_deref(), Some("USB-C Adapter"));
    }

    #[test]
    fn announces_plugging_the_chosen_microphone_back_in() {
        let unplugged = resolve_active(Some("USB-C Adapter"), &snapshot("MacBook Air Microphone", &["MacBook Air Microphone"]));
        let plugged = resolve_active(
            Some("USB-C Adapter"),
            &snapshot("USB-C Adapter", &["MacBook Air Microphone", "USB-C Adapter"]),
        );
        assert_eq!(
            switch_message(&unplugged, &plugged, Some("USB-C Adapter")),
            Some("Microphone switched to USB-C Adapter.".to_string())
        );
        assert_eq!(
            switch_message(&plugged, &unplugged, Some("USB-C Adapter")),
            Some("USB-C Adapter is unplugged. Recording from MacBook Air Microphone.".to_string())
        );
    }

    #[test]
    fn says_nothing_when_the_active_microphone_is_unchanged() {
        let snap = snapshot("USB-C Adapter", &["USB-C Adapter"]);
        let active = resolve_active(None, &snap);
        assert_eq!(switch_message(&active, &active, None), None);
    }

    #[test]
    fn stays_quiet_on_the_first_reading_after_launch() {
        let nothing = ActiveInput { name: None, falling_back: false };
        let first = resolve_active(None, &snapshot("USB-C Adapter", &["USB-C Adapter"]));
        assert_eq!(switch_message(&nothing, &first, None), None);
    }
}
