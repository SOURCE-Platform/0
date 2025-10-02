use super::Platform;
use std::path::PathBuf;

pub struct LinuxPlatform;

impl LinuxPlatform {
    pub fn new() -> Self {
        Self
    }

    /// Read /etc/os-release to get distribution info
    #[cfg(target_os = "linux")]
    fn read_os_release(&self) -> std::collections::HashMap<String, String> {
        use std::collections::HashMap;
        use std::fs;

        let mut map = HashMap::new();

        if let Ok(content) = fs::read_to_string("/etc/os-release") {
            for line in content.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    let value = value.trim_matches('"').to_string();
                    map.insert(key.to_string(), value);
                }
            }
        }

        map
    }

    /// Get machine ID from /etc/machine-id or /var/lib/dbus/machine-id
    #[cfg(target_os = "linux")]
    fn get_machine_id_internal(&self) -> Result<String, Box<dyn std::error::Error>> {
        use std::fs;

        // Try /etc/machine-id first (systemd standard)
        if let Ok(id) = fs::read_to_string("/etc/machine-id") {
            return Ok(id.trim().to_string());
        }

        // Fallback to dbus machine-id
        if let Ok(id) = fs::read_to_string("/var/lib/dbus/machine-id") {
            return Ok(id.trim().to_string());
        }

        Err("Could not read machine ID".into())
    }

    #[cfg(not(target_os = "linux"))]
    fn read_os_release(&self) -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    #[cfg(not(target_os = "linux"))]
    fn get_machine_id_internal(&self) -> Result<String, Box<dyn std::error::Error>> {
        Err("Not running on Linux".into())
    }
}

impl Platform for LinuxPlatform {
    fn get_os_name(&self) -> String {
        let os_release = self.read_os_release();

        // Try PRETTY_NAME first, then NAME, then fallback
        os_release
            .get("PRETTY_NAME")
            .or_else(|| os_release.get("NAME"))
            .cloned()
            .unwrap_or_else(|| "Linux".to_string())
    }

    fn get_os_version(&self) -> String {
        let os_release = self.read_os_release();

        // Try VERSION first, then VERSION_ID
        os_release
            .get("VERSION")
            .or_else(|| os_release.get("VERSION_ID"))
            .cloned()
            .unwrap_or_else(|| {
                // Fallback to uname
                #[cfg(target_os = "linux")]
                {
                    use std::process::Command;
                    Command::new("uname")
                        .arg("-r")
                        .output()
                        .ok()
                        .and_then(|output| String::from_utf8(output.stdout).ok())
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "Unknown".to_string())
                }
                #[cfg(not(target_os = "linux"))]
                {
                    "Unknown".to_string()
                }
            })
    }

    fn get_device_id(&self) -> Result<String, Box<dyn std::error::Error>> {
        self.get_machine_id_internal()
    }

    fn supports_screen_recording(&self) -> bool {
        // Linux supports screen recording via:
        // - X11: XShm, XDamage
        // - Wayland: PipeWire (modern), wlroots screencopy protocol
        // Most modern distributions support at least one method
        true
    }

    fn get_data_directory(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Follow XDG Base Directory specification
        let home = std::env::var("HOME")
            .map_err(|_| "Could not determine home directory")?;

        let mut path = PathBuf::from(home);
        path.push(".observer_data");

        Ok(path)
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;

    #[test]
    fn test_linux_platform_creation() {
        let platform = LinuxPlatform::new();
        let os_name = platform.get_os_name();

        assert!(!os_name.is_empty(), "OS name should not be empty");
        // Should contain Linux or a distribution name
        assert!(
            os_name.to_lowercase().contains("linux")
                || os_name.contains("Ubuntu")
                || os_name.contains("Debian")
                || os_name.contains("Fedora")
                || os_name.contains("Arch"),
            "OS name should be Linux-related, got: {}",
            os_name
        );
    }

    #[test]
    fn test_linux_version() {
        let platform = LinuxPlatform::new();
        let version = platform.get_os_version();

        assert!(!version.is_empty(), "Version should not be empty");
    }

    #[test]
    fn test_linux_device_id() {
        let platform = LinuxPlatform::new();
        let device_id = platform.get_device_id();

        assert!(device_id.is_ok(), "Should be able to get device ID");

        if let Ok(id) = device_id {
            assert!(!id.is_empty(), "Device ID should not be empty");
            // machine-id is typically 32 hex characters
            assert!(id.len() >= 32, "Device ID should be at least 32 characters");
        }
    }

    #[test]
    fn test_linux_supports_screen_recording() {
        let platform = LinuxPlatform::new();
        assert!(
            platform.supports_screen_recording(),
            "Linux should support screen recording"
        );
    }

    #[test]
    fn test_linux_data_directory() {
        let platform = LinuxPlatform::new();
        let data_dir = platform.get_data_directory();

        assert!(data_dir.is_ok(), "Should be able to get data directory");

        if let Ok(dir) = data_dir {
            let dir_str = dir.to_string_lossy();
            assert!(dir_str.contains(".observer_data"), "Should contain .observer_data");
            assert!(dir.is_absolute(), "Should be an absolute path");
        }
    }

    #[test]
    fn test_linux_os_release_parsing() {
        let platform = LinuxPlatform::new();
        let os_release = platform.read_os_release();

        // Should have at least some fields if /etc/os-release exists
        // (it exists on all modern systemd-based systems)
        if !os_release.is_empty() {
            assert!(
                os_release.contains_key("NAME") || os_release.contains_key("PRETTY_NAME"),
                "Should have NAME or PRETTY_NAME field"
            );
        }
    }
}
