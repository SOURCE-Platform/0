// Platform-specific audio capture for Linux
// Uses cpal for microphone and PulseAudio monitor sources for loopback

use crate::models::audio::{AudioDevice, AudioDeviceType, AudioError, AudioResult};
use cpal::traits::{DeviceTrait, HostTrait};

pub struct LinuxAudioCapture {
    device: Option<cpal::Device>,
    stream: Option<cpal::Stream>,
}

impl LinuxAudioCapture {
    pub fn new() -> AudioResult<Self> {
        Ok(Self {
            device: None,
            stream: None,
        })
    }

    /// Enumerate all audio devices
    pub fn enumerate_devices() -> AudioResult<Vec<AudioDevice>> {
        let mut devices = Vec::new();

        // Use cpal to enumerate input devices
        let host = cpal::default_host();
        let default_input = host.default_input_device();

        // Enumerate all input devices
        let input_devices = host.input_devices()
            .map_err(|e| AudioError::DeviceEnumerationError(format!("Failed to enumerate input devices: {}", e)))?;

        for device in input_devices {
            let name = device.name()
                .unwrap_or_else(|_| "Unknown Device".to_string());

            // Skip monitor sources - we'll add them separately
            let is_monitor = name.contains(".monitor") || name.contains("Monitor of");

            // Get default config to extract sample rate and channels
            let default_config = device.default_input_config()
                .map_err(|e| AudioError::DeviceEnumerationError(format!("Failed to get device config: {}", e)))?;

            let is_default = default_input.as_ref()
                .and_then(|d| d.name().ok())
                .map(|n| n == name)
                .unwrap_or(false);

            if is_monitor {
                // This is a monitor source (loopback)
                devices.push(AudioDevice {
                    id: name.clone(),
                    name: format!("{} (System Loopback)", name),
                    device_type: AudioDeviceType::SystemLoopback,
                    is_default: false,
                    sample_rate: default_config.sample_rate().0,
                    channels: default_config.channels() as u32,
                });
            } else {
                // Regular input device (microphone)
                devices.push(AudioDevice {
                    id: name.clone(),
                    name: name.clone(),
                    device_type: AudioDeviceType::Microphone,
                    is_default,
                    sample_rate: default_config.sample_rate().0,
                    channels: default_config.channels() as u32,
                });
            }
        }

        Ok(devices)
    }

    /// Start capturing from a device
    pub fn start_capture(&mut self, device_id: &str) -> AudioResult<()> {
        let host = cpal::default_host();

        // Find the device by name/id
        let device = if device_id == "default" {
            host.default_input_device()
                .ok_or_else(|| AudioError::DeviceNotFound("No default input device".to_string()))?
        } else {
            // Search through input devices for matching name
            let devices = host.input_devices()
                .map_err(|e| AudioError::DeviceEnumerationError(format!("Failed to enumerate devices: {}", e)))?;

            devices
                .filter(|d| d.name().map(|n| n == device_id || n.contains(device_id)).unwrap_or(false))
                .next()
                .ok_or_else(|| AudioError::DeviceNotFound(format!("Device not found: {}", device_id)))?
        };

        let config = device.default_input_config()
            .map_err(|e| AudioError::DeviceConfigError(format!("Failed to get config: {}", e)))?;

        let is_monitor = device_id.contains(".monitor") || device_id.contains("Monitor of");

        println!("Starting Linux audio capture for device: {} ({}Hz, {} channels, monitor: {})",
            device_id, config.sample_rate().0, config.channels(), is_monitor);

        self.device = Some(device);
        // Note: PulseAudio monitor sources work like regular input sources
        // No special handling needed compared to microphones

        Ok(())
    }

    /// Stop capturing
    pub fn stop_capture(&mut self) -> AudioResult<()> {
        self.stream = None;
        self.device = None;
        println!("Stopped Linux audio capture");
        Ok(())
    }

    /// Get audio samples (non-blocking)
    pub fn read_samples(&mut self) -> AudioResult<Vec<f32>> {
        // TODO: Implement ring buffer for audio samples
        // This will be populated by the cpal stream callback
        Ok(vec![])
    }
}

// PulseAudio monitor helper
pub fn get_monitor_sources() -> AudioResult<Vec<String>> {
    // TODO: List all monitor sources (sink monitors)
    // These are the loopback sources for system audio

    Ok(vec!["default.monitor".to_string()])
}
