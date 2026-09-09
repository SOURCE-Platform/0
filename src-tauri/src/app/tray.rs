use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

pub const TRAY_ID: &str = "source-tray";
pub const MENU_OPEN_ID: &str = "tray-open";
pub const MENU_QUIT_ID: &str = "tray-quit";

/// Show (and focus) the main window.
/// Recovers from a macOS app-level hide as well.
pub fn show_main_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.show();
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Hide the main window but keep the tray icon (and process) alive.
pub fn hide_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

/// Build the menu-bar tray icon: circle template + Open / Quit menu.
///
/// `icon_as_template(true)` makes macOS render the mask automatically:
/// dark circle in light mode, white circle in dark mode.
pub fn build_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let open_item = MenuItem::with_id(app, MENU_OPEN_ID, "Open", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &quit_item])?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../../icons/tray-circle@2x.png"))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("SOURCE")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_OPEN_ID => show_main_window(app),
            MENU_QUIT_ID => request_quit(app),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Graceful quit from the tray: ask the dictation helper to shut down,
/// then let Tauri tear down windows, servers, and recorders cleanly.
///
/// Using `app.exit` (instead of killing the process) avoids the macOS
/// "quit unexpectedly" crash reporter dialog.
pub fn request_quit(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<super::state::AppState>() {
        if let Some(sender) = state.dictation_commands.blocking_lock().clone() {
            let _ = sender.try_send(crate::core::multimodal::SupervisorCommand::Shutdown);
        }
    }
    app.exit(0);
}
