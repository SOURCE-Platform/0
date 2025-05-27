// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Application modules
mod config;

// Workspace crates
use observer_core; // Added for workspace integration

// Import necessary Tauri modules
use tauri::{
    AppHandle, CustomMenuItem, Manager, Menu, MenuItem, State, Submenu, SystemTray,
    SystemTrayEvent, SystemTrayMenu, SystemTrayMenuItem,
};
use std::sync::{Arc, Mutex};

// --- Tauri Commands ---

#[tauri::command]
async fn greet(name: String) -> Result<String, ()> {
    tracing::info!("Greet command called with name: '{}'", name);
    if name.is_empty() {
        tracing::warn!("Greet command called with empty name.");
        return Err(()); 
    }
    Ok(format!("Hello, {}! You've been greeted from Rust!", name))
}

#[tauri::command]
async fn get_app_settings(
    settings_state: State<'_, Arc<Mutex<config::AppSettings>>>,
) -> Result<config::AppSettings, String> {
    tracing::info!("Command 'get_app_settings' called");
    match settings_state.lock() {
        Ok(settings_guard) => {
            tracing::debug!("Settings retrieved: {:?}", *settings_guard);
            Ok(settings_guard.clone())
        },
        Err(e) => {
            let error_msg = format!("Failed to acquire lock for get_app_settings: {}", e);
            tracing::error!("{}", error_msg);
            Err(error_msg)
        }
    }
}

#[tauri::command]
async fn set_app_setting_theme(
    theme: String,
    settings_state: State<'_, Arc<Mutex<config::AppSettings>>>,
) -> Result<(), String> {
    tracing::info!("Command 'set_app_setting_theme' called with theme: {}", theme);
    match settings_state.lock() {
        Ok(mut settings_guard) => {
            settings_guard.theme = Some(theme.clone());
            tracing::debug!("Theme in memory updated to: {:?}", settings_guard.theme);
            
            if let Err(save_err) = config::save_settings(&*settings_guard) {
                tracing::error!("Failed to save settings after theme update: {}", save_err);
                return Err(format!("Failed to save settings: {}", save_err));
            }
            tracing::info!("Theme updated to '{}' and settings successfully saved.", theme);
            Ok(())
        }
        Err(e) => {
            let error_msg = format!("Failed to acquire lock for set_app_setting_theme: {}", e);
            tracing::error!("{}", error_msg);
            Err(error_msg)
        }
    }
}

#[tauri::command]
async fn get_observer_info() -> Result<String, ()> {
    tracing::info!("Command 'get_observer_info' called, interacting with observer_core crate.");
    // This directly calls a function from the observer_core workspace member.
    // Error handling would be more robust in a real application if get_observer_status could fail.
    // For this placeholder, direct Ok is fine as get_observer_status returns String directly.
    Ok(observer_core::get_observer_status())
}

// --- Main Application Setup ---
fn main() {
    // Initialize tracing subscriber for logging. This should be one of the first things.
    tracing_subscriber::fmt::init();
    tracing::info!("Application is starting up...");

    // Load initial settings from config file or defaults.
    let loaded_settings = config::load_settings();
    tracing::info!("Initial application settings loaded: {:?}", loaded_settings);
    // Wrap settings in Arc<Mutex<T>> for thread-safe shared state managed by Tauri.
    let settings_state = Arc::new(Mutex::new(loaded_settings));

    // --- Menu Definition ---
    let file_submenu = Submenu::new("File", Menu::new().add_native_item(MenuItem::CloseWindow));
    let edit_submenu = Submenu::new(
        "Edit",
        Menu::new()
            .add_native_item(MenuItem::Copy)
            .add_native_item(MenuItem::Paste)
            .add_native_item(MenuItem::Separator)
            .add_native_item(MenuItem::Cut)
            .add_native_item(MenuItem::SelectAll),
    );
    let view_submenu = Submenu::new("View", Menu::new()); // Placeholder for view options
    let about_item = CustomMenuItem::new("about".to_string(), "About Observer App");
    let help_submenu = Submenu::new("Help", Menu::new().add_item(about_item));
    
    let app_menu = Menu::new()
        .add_submenu(file_submenu)
        .add_submenu(edit_submenu)
        .add_submenu(view_submenu)
        .add_submenu(help_submenu);

    // --- System Tray Definition ---
    let tray_menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("open".to_string(), "Open App"))
        .add_native_item(SystemTrayMenuItem::Separator) // Visual separator
        .add_item(CustomMenuItem::new("quit".to_string(), "Quit App"));
    let system_tray = SystemTray::new().with_menu(tray_menu);

    // --- Tauri App Builder ---
    // Construct the Tauri application builder, configuring menus, system tray, state, and handlers.
    tauri::Builder::default()
        .manage(settings_state.clone()) // Manage the AppSettings state.
        .menu(app_menu)
        .on_menu_event(|event| {
            tracing::info!("Menu event: id={}", event.menu_item_id());
            match event.menu_item_id() {
                "about" => {
                    tracing::info!("'About Observer App' menu item activated.");
                    // Emit an event to the frontend to handle showing an "About" dialog or page.
                    event.window().emit("show-about-dialog", ()).unwrap_or_else(|e| {
                        tracing::error!("Failed to emit 'show-about-dialog' event: {}", e);
                    });
                }
                _ => {
                    tracing::debug!("Unhandled menu event for ID: {}", event.menu_item_id());
                }
            }
        })
        .system_tray(system_tray)
        .on_system_tray_event(|app: &AppHandle, event| {
            match event {
                SystemTrayEvent::MenuItemClick { id, .. } => {
                    let item_id = id.as_str();
                    tracing::info!("System tray menu item '{}' clicked.", item_id);
                    match item_id {
                        "open" => {
                            if let Some(window) = app.get_window("main") {
                                if let Err(e) = window.show() { tracing::error!("Failed to show main window from tray: {}", e); }
                                if let Err(e) = window.set_focus() { tracing::error!("Failed to focus main window from tray: {}", e); }
                            } else { tracing::warn!("Could not get main window on tray 'open'."); }
                        }
                        "quit" => {
                            tracing::info!("'Quit App' from tray. Exiting application.");
                            app.exit(0); // Gracefully exits the application.
                        }
                        _ => {
                            tracing::debug!("Unhandled system tray menu item click for ID: {}", item_id);
                        }
                    }
                }
                SystemTrayEvent::LeftClick { .. } => {
                    tracing::info!("System tray icon left-clicked (toggle window visibility).");
                    if let Some(window) = app.get_window("main") {
                        match window.is_visible() {
                            Ok(true) => { 
                                tracing::debug!("Main window is visible, attempting to hide.");
                                if let Err(e) = window.hide() { tracing::error!("Failed to hide main window: {}", e); }
                            }
                            Ok(false) => {
                                tracing::debug!("Main window is hidden, attempting to show and focus.");
                                if let Err(e) = window.show() { tracing::error!("Failed to show main window: {}", e); }
                                if let Err(e) = window.set_focus() { tracing::error!("Failed to focus main window: {}", e); }
                            }
                            Err(e) => { tracing::error!("Window visibility check failed: {}", e); }
                        }
                    } else { tracing::warn!("Could not get main window on tray left-click."); }
                }
                _ => {
                    tracing::trace!("Unhandled system tray event: {:?}", event); // Trace for less common events
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_app_settings,
            set_app_setting_theme,
            get_observer_info // Added new command for workspace integration
        ])
        .run(tauri::generate_context!()) // Generates context and runs the application.
        .expect("error while running tauri application");

    // This log message might not be reached if the application exits via app.exit(0)
    // or if all windows are closed and exit_on_all_closed is true (default).
    tracing::info!("Application has finished its run method (this may indicate an issue if unexpected).");
}
