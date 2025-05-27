// src-tauri/src/config.rs
//! Manages application settings, including loading from and saving to a TOML file.

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use tauri::api::path::config_dir; // For Tauri v1

/// Defines the structure of the application's settings.
/// These settings are loaded at startup and can be modified during runtime.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppSettings {
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub theme: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        tracing::debug!("Initializing default AppSettings.");
        AppSettings {
            window_width: Some(1024),
            window_height: Some(768),
            theme: Some("dark".to_string()),
        }
    }
}

/// Determines the platform-specific path for the settings.toml file.
/// Ensures the application-specific configuration directory exists.
pub fn get_config_path() -> Result<PathBuf, String> {
    tracing::trace!("Attempting to determine configuration path.");
    let mut path = config_dir().ok_or_else(|| {
        let err_msg = "Failed to get config directory (tauri::api::path::config_dir returned None).";
        tracing::error!("{}", err_msg);
        err_msg.to_string()
    })?;
    
    path.push("com.source.observerapp"); // Application-specific subdirectory
    
    if !path.exists() {
        tracing::debug!("Config directory '{}' does not exist, attempting to create.", path.display());
        fs::create_dir_all(&path).map_err(|e| {
            let err_msg = format!("Failed to create config subdirectory '{}': {}", path.display(), e);
            tracing::error!("{}", err_msg);
            err_msg
        })?;
        tracing::info!("Successfully created config directory at: {}", path.display());
    }
    
    path.push("settings.toml");
    tracing::trace!("Determined configuration file path: {}", path.display());
    Ok(path)
}

/// Loads settings from the settings.toml file.
/// If the file doesn't exist, or if there's an error loading or parsing,
/// default settings are returned and an attempt is made to save them.
pub fn load_settings() -> AppSettings {
    tracing::debug!("Attempting to load application settings.");
    match get_config_path() {
        Ok(path) => {
            if path.exists() {
                tracing::info!("Found settings file at: {}. Attempting to load.", path.display());
                match File::open(&path) {
                    Ok(mut file) => {
                        let mut contents = String::new();
                        if let Err(e) = file.read_to_string(&mut contents) {
                            tracing::warn!("Failed to read settings file at {}: {}. Using default settings.", path.display(), e);
                        } else {
                            match toml::from_str(&contents) {
                                Ok(settings) => {
                                    tracing::info!("Successfully loaded settings from {}.", path.display());
                                    return settings;
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to parse TOML from settings file at {}: {}. Using default settings.", path.display(), e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to open settings file at {}: {}. Using default settings.", path.display(), e);
                    }
                }
            } else {
                tracing::info!("Settings file not found at {}. Using default settings and attempting to create one.", path.display());
                let default_settings = AppSettings::default();
                if let Err(e) = save_settings(&default_settings) {
                    tracing::warn!("Failed to save initial default settings to {}: {}", path.display(), e);
                } else {
                    tracing::info!("Successfully saved default settings to {}.", path.display());
                }
                return default_settings; // Return defaults after attempting to save
            }
        }
        Err(e) => {
            // Error already logged by get_config_path if it failed there.
            tracing::warn!("Failed to get config path for loading: {}. Using default settings.", e);
        }
    }
    tracing::warn!("Falling back to default settings due to previous errors.");
    AppSettings::default() // Fallback default if all else fails
}

/// Saves the provided AppSettings to the settings.toml file.
pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    tracing::debug!("Attempting to save application settings to file.");
    let path = get_config_path()?; // Error already logged by get_config_path if it fails.
    tracing::info!("Saving settings to: {}", path.display());

    let toml_string = toml::to_string_pretty(&settings).map_err(|e| {
        let err_msg = format!("Failed to serialize settings to TOML: {}", e);
        tracing::error!("{}", err_msg);
        err_msg
    })?;
    
    let mut file = File::create(&path).map_err(|e| {
        let err_msg = format!("Failed to create/truncate settings file at {}: {}", path.display(), e);
        tracing::error!("{}", err_msg);
        err_msg
    })?;
    
    file.write_all(toml_string.as_bytes()).map_err(|e| {
        let err_msg = format!("Failed to write settings to {}: {}", path.display(), e);
        tracing::error!("{}", err_msg);
        err_msg
    })?;
    
    tracing::info!("Successfully saved settings to {}", path.display());
    Ok(())
}
