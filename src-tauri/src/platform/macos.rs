use super::Platform;
use std::path::PathBuf;
use std::process::Command;

pub struct MacOSPlatform;

impl MacOSPlatform {
    pub fn new() -> Self {
        Self
    }

    /// Get the macOS version using sw_vers command
    fn get_sw_vers_output(&self, flag: &str) -> String {
        Command::new("sw_vers")
            .arg(flag)
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// Get hardware UUID using system_profiler
    fn get_hardware_uuid(&self) -> Result<String, Box<dyn std::error::Error>> {
        let output = Command::new("system_profiler")
            .arg("SPHardwareDataType")
            .output()?;

        let stdout = String::from_utf8(output.stdout)?;

        // Parse the Hardware UUID line
        for line in stdout.lines() {
            if line.trim().starts_with("Hardware UUID:") {
                if let Some(uuid) = line.split(':').nth(1) {
                    return Ok(uuid.trim().to_string());
                }
            }
        }

        Err("Could not find Hardware UUID".into())
    }
}

impl Platform for MacOSPlatform {
    fn get_os_name(&self) -> String {
        self.get_sw_vers_output("-productName")
    }

    fn get_os_version(&self) -> String {
        let version = self.get_sw_vers_output("-productVersion");
        let build = self.get_sw_vers_output("-buildVersion");

        if build != "Unknown" {
            format!("{} (Build {})", version, build)
        } else {
            version
        }
    }

    fn get_device_id(&self) -> Result<String, Box<dyn std::error::Error>> {
        // Try to get Hardware UUID first (most stable)
        if let Ok(uuid) = self.get_hardware_uuid() {
            return Ok(uuid);
        }

        // Fallback: use IOPlatformUUID
        let output = Command::new("ioreg")
            .args(["-d2", "-c", "IOPlatformExpertDevice"])
            .output()?;

        let stdout = String::from_utf8(output.stdout)?;

        for line in stdout.lines() {
            if line.contains("IOPlatformUUID") {
                if let Some(uuid_part) = line.split('=').nth(1) {
                    let uuid = uuid_part
                        .trim()
                        .trim_matches('"')
                        .trim()
                        .to_string();
                    return Ok(uuid);
                }
            }
        }

        Err("Could not determine device ID".into())
    }

    fn supports_screen_recording(&self) -> bool {
        // macOS supports screen recording via AVFoundation and ScreenCaptureKit
        // Available on macOS 10.15+ (Catalina) for AVFoundation
        // ScreenCaptureKit requires macOS 12.3+
        true
    }

    fn get_data_directory(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let home = std::env::var("HOME")
            .map_err(|_| "Could not determine home directory")?;

        let mut path = PathBuf::from(home);
        path.push(".observer_data");

        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macos_platform_creation() {
        let platform = MacOSPlatform::new();
        let os_name = platform.get_os_name();

        // Should contain macOS or Mac OS X
        assert!(
            os_name.to_lowercase().contains("mac") || os_name.contains("Darwin"),
            "OS name should be macOS-related, got: {}",
            os_name
        );
    }

    #[test]
    fn test_macos_version() {
        let platform = MacOSPlatform::new();
        let version = platform.get_os_version();

        assert!(!version.is_empty(), "Version should not be empty");
        // Should contain a version number
        assert!(
            version.chars().any(|c| c.is_numeric()),
            "Version should contain numbers, got: {}",
            version
        );
    }

    #[test]
    fn test_macos_device_id() {
        let platform = MacOSPlatform::new();
        let device_id = platform.get_device_id();

        assert!(device_id.is_ok(), "Should be able to get device ID");

        if let Ok(id) = device_id {
            assert!(!id.is_empty(), "Device ID should not be empty");
            // UUID format check (should contain hyphens)
            assert!(id.contains('-'), "Device ID should be in UUID format");
        }
    }

    #[test]
    fn test_macos_supports_screen_recording() {
        let platform = MacOSPlatform::new();
        assert!(
            platform.supports_screen_recording(),
            "macOS should support screen recording"
        );
    }

    #[test]
    fn test_macos_data_directory() {
        let platform = MacOSPlatform::new();
        let data_dir = platform.get_data_directory();

        assert!(data_dir.is_ok(), "Should be able to get data directory");

        if let Ok(dir) = data_dir {
            let dir_str = dir.to_string_lossy();
            assert!(dir_str.contains(".observer_data"), "Should contain .observer_data");
            assert!(dir.is_absolute(), "Should be an absolute path");
        }
    }
}
