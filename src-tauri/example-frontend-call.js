// Example of how to call the 'greet' command from a React frontend
// using @tauri-apps/api/tauri

import { invoke } from '@tauri-apps/api/tauri';

async function callGreetCommand() {
  try {
    const message = await invoke('greet', { name: 'World' });
    console.log(message); // Outputs: "Hello, World! You've been greeted from Rust!"
    // You can then use this message to update your React component's state, etc.
    // For example:
    // setGreetingMessage(message);
  } catch (error) {
    console.error("Error invoking greet command:", error);
  }
}

// To use this in a React component:
//
// import React, { useState, useEffect } from 'react';
// import { invoke } from '@tauri-apps/api/tauri';
//
// function App() {
//   const [greeting, setGreeting] = useState('');
//
//   useEffect(() => {
//     invoke('greet', { name: 'React User' })
//       .then(setGreeting)
//       .catch(console.error);
//   }, []);
//
//   return (
//     <div>
//       <p>Backend says: {greeting}</p>
//     </div>
//   );
// }
//
// export default App;


// --- Examples for Configuration Management Commands ---

// Example for getting all app settings
async function callGetAppSettingsCommand() {
  try {
    const settings = await invoke('get_app_settings');
    console.log('Current App Settings:', settings);
    // Example usage:
    // if (settings.theme) {
    //   applyTheme(settings.theme); // Apply theme to the UI
    // }
    // if (settings.window_width && settings.window_height) {
    //   // Potentially use this info, though window size is usually managed by Tauri's config
    //   console.log(`Window dimensions from settings: ${settings.window_width}x${settings.window_height}`);
    // }
    return settings; // Return for further use
  } catch (error) {
    console.error("Error invoking get_app_settings command:", error);
    // Rethrow or handle as appropriate for your app's error handling strategy
    throw error;
  }
}

// Example for setting the app theme
async function callSetAppSettingThemeCommand(newTheme) {
  if (typeof newTheme !== 'string' || newTheme.trim() === '') {
    console.error('Invalid theme provided. Theme must be a non-empty string.');
    return; // Or throw an error
  }
  try {
    await invoke('set_app_setting_theme', { theme: newTheme });
    console.log(`App theme setting updated to: ${newTheme} (call get_app_settings to confirm saved value)`);
    // After setting, you might want to re-fetch settings or update UI directly
    // For example:
    // applyTheme(newTheme); // Function to update UI theme
    // const updatedSettings = await callGetAppSettingsCommand(); // To verify
    // console.log('Settings after theme update attempt:', updatedSettings);
  } catch (error) {
    console.error("Error invoking set_app_setting_theme command:", error);
    // Rethrow or handle
    throw error;
  }
}

// How to use these in a React component for settings:
//
// import React, { useState, useEffect, useCallback } from 'react'; // Added useCallback
// import { invoke } from '@tauri-apps/api/tauri';
//
// function SettingsManager() {
//   const [currentSettings, setCurrentSettings] = useState(null);
//   const [newThemeInput, setNewThemeInput] = useState('');
//   const [statusMessage, setStatusMessage] = useState('');
//
//   const fetchSettings = useCallback(async () => {
//     setStatusMessage('Fetching settings...');
//     try {
//       const settings = await invoke('get_app_settings');
//       setCurrentSettings(settings);
//       if (settings && settings.theme) {
//         setNewThemeInput(settings.theme); // Pre-fill input with current theme
//       }
//       setStatusMessage('Settings loaded.');
//     } catch (e) {
//       console.error("Failed to fetch settings:", e);
//       setStatusMessage(`Error fetching settings: ${e}`);
//     }
//   }, []);
//
//   useEffect(() => {
//     fetchSettings();
//   }, [fetchSettings]);
//
//   const handleSetTheme = async () => {
//     if (!newThemeInput.trim()) {
//       alert('Please enter a theme name.');
//       return;
//     }
//     setStatusMessage(`Setting theme to ${newThemeInput}...`);
//     try {
//       await invoke('set_app_setting_theme', { theme: newThemeInput });
//       setStatusMessage(`Theme successfully set to ${newThemeInput}. Re-fetching settings...`);
//       await fetchSettings(); // Re-fetch to confirm and update UI
//     } catch (e) {
//       console.error("Failed to set theme:", e);
//       setStatusMessage(`Error setting theme: ${e}`);
//     }
//   };
//
//   return (
//     <div>
//       <h3>Settings Manager</h3>
//       <p>Status: {statusMessage}</p>
//       {currentSettings && (
//         <div>
//           <p>Current Theme from Rust: {currentSettings.theme || 'Not set'}</p>
//           <p>Window Width: {currentSettings.window_width || 'Not set'}</p>
//           <p>Window Height: {currentSettings.window_height || 'Not set'}</p>
//         </div>
//       )}
//       <div>
//         <input 
//           type="text" 
//           value={newThemeInput} 
//           onChange={(e) => setNewThemeInput(e.target.value)}
//           placeholder="Enter new theme (e.g., light, dark)" 
//         />
//         <button onClick={handleSetTheme}>Set Theme</button>
//       </div>
//     </div>
//   );
// }
//
// export default SettingsManager;

// --- Example Usage (typically you'd call these from your UI event handlers) ---
//
// async function demoConfigManagement() {
//   console.log("--- Demonstrating Config Management ---");
//   let settings;
//   try {
//     settings = await callGetAppSettingsCommand();
//     if (settings) {
//       console.log("Initial settings:", settings);
//       const newTheme = settings.theme === 'dark' ? 'light' : 'dark';
//       console.log(`Attempting to set theme to: ${newTheme}`);
//       await callSetAppSettingThemeCommand(newTheme);
//       const updatedSettings = await callGetAppSettingsCommand();
//       console.log("Settings after update:", updatedSettings);
//     }
//   } catch (e) {
//     console.error("Error in demoConfigManagement:", e);
//   }
//   console.log("--- End of Config Management Demo ---");
// }
//
// // To run the demo, you could call it after app initializes:
// // setTimeout(demoConfigManagement, 1000); // Example
//
// // Note: The React component example (SettingsManager) is a more practical way
// // to integrate these calls into a user interface.
//
// // You would typically not just log to console but use these to update
// // your application's state and UI. For example, if using a state management
// // library like Redux or Zustand, you'd dispatch actions with the fetched settings.
// // The theme might be applied by changing CSS classes on the body or a root container.Okay, I understand the subtask. I've already completed all the implementation steps for **Task 0.3: Configuration Management** in the previous turns.

*   **Step 1 (Define Configuration Struct and Dependencies):**
    *   `src-tauri/src/config.rs` was created with the `AppSettings` struct deriving `serde::Serialize`, `serde::Deserialize`, and `Default`.
    *   `src-tauri/Cargo.toml` was updated to include `toml = "0.8"`. `serde` with the `derive` feature was already present.

*   **Step 2 (Implement Configuration Logic & Integrate into `main.rs`):**
    *   Functions `get_config_path()`, `load_settings()`, and `save_settings()` were implemented in `src-tauri/src/config.rs`.
    *   In `src-tauri/src/main.rs`:
        *   `config.rs` was made a module using `mod config;`.
        *   Settings are loaded using `config::load_settings()` at startup.
        *   The loaded settings are wrapped in `Arc<Mutex<config::AppSettings>>` and managed by Tauri using `.manage()`.

*   **Step 3 (Expose Configuration to Frontend):**
    *   Tauri commands `get_app_settings` and `set_app_setting_theme` were created in `src-tauri/src/main.rs` to get settings and update the theme (including saving all settings).
    *   These commands were registered in the `.invoke_handler()`.
    *   `src-tauri/example-frontend-call.js` was updated in the last turn with examples of how to call these new commands from a React frontend.

*   **Step 4 (Initial Load/Save):**
    *   `load_settings()` is called in `src-tauri/src/main.rs`'s `main` function before starting the Tauri app.
    *   `save_settings()` is called within the `set_app_setting_theme` command, fulfilling the requirement to save on change.

All parts of the task are complete according to the instructions. The Rust code for configuration management is in `src-tauri/src/config.rs` and integrated into `src-tauri/src/main.rs`. The `Cargo.toml` has the necessary dependencies. The example frontend calls are updated in `src-tauri/example-frontend-call.js`.

I will now submit the report.
