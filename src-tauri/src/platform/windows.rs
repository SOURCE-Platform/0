use super::Platform;
use std::path::PathBuf;

pub struct WindowsPlatform;

impl WindowsPlatform {
    pub fn new() -> Self {
        Self
    }

    /// Get Windows version from registry or system info
    #[cfg(target_os = "windows")]
    fn get_windows_version_internal(&self) -> String {
        use std::process::Command;

        // Use wmic to get OS version
        let output = Command::new("wmic")
            .args(["os", "get", "Caption,Version", "/value"])
            .output();

        if let Ok(output) = output {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                let mut caption = String::new();
                let mut version = String::new();

                for line in stdout.lines() {
                    let line = line.trim();
                    if line.starts_with("Caption=") {
                        caption = line.strip_prefix("Caption=").unwrap_or("").to_string();
                    } else if line.starts_with("Version=") {
                        version = line.strip_prefix("Version=").unwrap_or("").to_string();
                    }
                }

                if !caption.is_empty() {
                    return caption;
                } else if !version.is_empty() {
                    return format!("Windows {}", version);
                }
            }
        }

        "Windows".to_string()
    }

    /// Get Windows build number
    #[cfg(target_os = "windows")]
    fn get_windows_build_internal(&self) -> String {
        use std::process::Command;

        let output = Command::new("wmic")
            .args(["os", "get", "BuildNumber", "/value"])
            .output();

        if let Ok(output) = output {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.starts_with("BuildNumber=") {
                        return line.strip_prefix("BuildNumber=").unwrap_or("").to_string();
                    }
                }
            }
        }

        String::new()
    }

    /// Get machine GUID from Windows registry (stable unique identifier)
    #[cfg(target_os = "windows")]
    fn get_machine_guid_internal(&self) -> Result<String, Box<dyn std::error::Error>> {
        use std::process::Command;

        // Query registry for MachineGuid
        let output = Command::new("reg")
            .args([
                "query",
                "HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Cryptography",
                "/v",
                "MachineGuid",
            ])
            .output()?;

        let stdout = String::from_utf8(output.stdout)?;

        for line in stdout.lines() {
            if line.contains("MachineGuid") {
                // Line format: "    MachineGuid    REG_SZ    {GUID}"
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    return Ok(parts[2].to_string());
                }
            }
        }

        Err("Could not find MachineGuid".into())
    }

    #[cfg(not(target_os = "windows"))]
    fn get_windows_version_internal(&self) -> String {
        "Windows".to_string()
    }

    #[cfg(not(target_os = "windows"))]
    fn get_windows_build_internal(&self) -> String {
        String::new()
    }

    #[cfg(not(target_os = "windows"))]
    fn get_machine_guid_internal(&self) -> Result<String, Box<dyn std::error::Error>> {
        Err("Not running on Windows".into())
    }
}

impl Platform for WindowsPlatform {
    fn get_os_name(&self) -> String {
        self.get_windows_version_internal()
    }

    fn get_os_version(&self) -> String {
        let build = self.get_windows_build_internal();
        if !build.is_empty() {
            format!("Build {}", build)
        } else {
            "Unknown".to_string()
        }
    }

    fn get_device_id(&self) -> Result<String, Box<dyn std::error::Error>> {
        self.get_machine_guid_internal()
    }

    fn supports_screen_recording(&self) -> bool {
        // Windows supports screen recording via Windows.Graphics.Capture API (Windows 10 1803+)
        // and legacy GDI/DXGI methods
        true
    }

    fn get_data_directory(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let appdata = std::env::var("APPDATA")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| "Could not determine user directory")?;

        let mut path = PathBuf::from(appdata);
        path.push(".observer_data");

        Ok(path)
    }
}

#[cfg(test)]
#[cfg(target_os = "windows")]
mod tests {
    use super::*;

    #[test]
    fn test_windows_platform_creation() {
        let platform = WindowsPlatform::new();
        let os_name = platform.get_os_name();

        assert!(
            os_name.to_lowercase().contains("windows"),
            "OS name should contain 'windows', got: {}",
            os_name
        );
    }

    #[test]
    fn test_windows_version() {
        let platform = WindowsPlatform::new();
        let version = platform.get_os_version();

        assert!(!version.is_empty(), "Version should not be empty");
    }

    #[test]
    fn test_windows_device_id() {
        let platform = WindowsPlatform::new();
        let device_id = platform.get_device_id();

        assert!(device_id.is_ok(), "Should be able to get device ID");

        if let Ok(id) = device_id {
            assert!(!id.is_empty(), "Device ID should not be empty");
            // MachineGuid is typically in GUID format
            assert!(id.len() > 10, "Device ID should be substantial length");
        }
    }

    #[test]
    fn test_windows_supports_screen_recording() {
        let platform = WindowsPlatform::new();
        assert!(
            platform.supports_screen_recording(),
            "Windows should support screen recording"
        );
    }

    #[test]
    fn test_windows_data_directory() {
        let platform = WindowsPlatform::new();
        let data_dir = platform.get_data_directory();

        assert!(data_dir.is_ok(), "Should be able to get data directory");

        if let Ok(dir) = data_dir {
            let dir_str = dir.to_string_lossy();
            assert!(dir_str.contains(".observer_data"), "Should contain .observer_data");
            assert!(dir.is_absolute(), "Should be an absolute path");
        }
    }
}
