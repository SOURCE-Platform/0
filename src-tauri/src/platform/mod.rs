use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

/// Platform abstraction trait for OS-specific operations
pub trait Platform: Send + Sync {
    /// Get the operating system name
    fn get_os_name(&self) -> String;

    /// Get the operating system version
    fn get_os_version(&self) -> String;

    /// Get a stable unique device identifier
    fn get_device_id(&self) -> Result<String, Box<dyn std::error::Error>>;

    /// Check if the platform supports screen recording
    fn supports_screen_recording(&self) -> bool;

    /// Get the default data directory for the application
    fn get_data_directory(&self) -> Result<PathBuf, Box<dyn std::error::Error>>;
}

/// Get the current platform implementation
pub fn get_platform() -> Box<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOSPlatform::new())
    }

    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsPlatform::new())
    }

    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxPlatform::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_platform() {
        let platform = get_platform();

        // Test that all methods return valid values
        let os_name = platform.get_os_name();
        assert!(!os_name.is_empty(), "OS name should not be empty");

        let os_version = platform.get_os_version();
        assert!(!os_version.is_empty(), "OS version should not be empty");

        let device_id = platform.get_device_id();
        assert!(device_id.is_ok(), "Device ID should be retrievable");
        if let Ok(id) = device_id {
            assert!(!id.is_empty(), "Device ID should not be empty");
        }

        let data_dir = platform.get_data_directory();
        assert!(data_dir.is_ok(), "Data directory should be retrievable");
        if let Ok(dir) = data_dir {
            assert!(dir.to_str().is_some(), "Data directory should be a valid path");
        }

        // Screen recording support is platform-dependent, just verify it returns a boolean
        let _ = platform.supports_screen_recording();
    }

    #[test]
    fn test_os_name_matches_target() {
        let platform = get_platform();
        let os_name = platform.get_os_name().to_lowercase();

        #[cfg(target_os = "macos")]
        assert!(os_name.contains("mac") || os_name.contains("darwin"));

        #[cfg(target_os = "windows")]
        assert!(os_name.contains("windows"));

        #[cfg(target_os = "linux")]
        assert!(os_name.contains("linux"));
    }

    #[test]
    fn test_device_id_consistency() {
        let platform = get_platform();

        // Device ID should be consistent across multiple calls
        let id1 = platform.get_device_id().expect("Should get device ID");
        let id2 = platform.get_device_id().expect("Should get device ID");

        assert_eq!(id1, id2, "Device ID should be stable across calls");
    }

    #[test]
    fn test_data_directory_exists_or_creatable() {
        let platform = get_platform();
        let data_dir = platform.get_data_directory().expect("Should get data directory");

        // Path should be absolute
        assert!(data_dir.is_absolute(), "Data directory should be an absolute path");

        // Should contain .observer_data
        assert!(
            data_dir.to_string_lossy().contains(".observer_data"),
            "Data directory should contain .observer_data"
        );
    }
}
