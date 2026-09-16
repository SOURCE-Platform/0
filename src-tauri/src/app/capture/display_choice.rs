use crate::models::capture::Display;

/// The display to record, given the one the app saved.
///
/// macOS renumbers displays when a monitor is plugged in or out, and sometimes
/// after a restart, so a saved number can point at a display that's gone. Then
/// record the main display rather than failing. None only when no display is
/// online at all.
pub fn online_display_or_main(displays: &[Display], saved: u32) -> Option<u32> {
    if displays.iter().any(|display| display.id == saved) {
        return Some(saved);
    }
    displays
        .iter()
        .find(|display| display.is_primary)
        .or_else(|| displays.first())
        .map(|display| display.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: u32, is_primary: bool) -> Display {
        Display { id, name: format!("Display {id}"), x: 0, y: 0, width: 1512, height: 982, is_primary }
    }

    #[test]
    fn keeps_the_saved_display_while_it_is_online() {
        assert_eq!(online_display_or_main(&[display(2, true), display(5, false)], 5), Some(5));
    }

    #[test]
    fn falls_back_to_the_main_display_when_the_saved_one_is_gone() {
        assert_eq!(online_display_or_main(&[display(5, false), display(2, true)], 1), Some(2));
    }

    #[test]
    fn uses_the_first_display_when_none_is_marked_main() {
        assert_eq!(online_display_or_main(&[display(7, false)], 1), Some(7));
        assert_eq!(online_display_or_main(&[], 1), None);
    }
}
