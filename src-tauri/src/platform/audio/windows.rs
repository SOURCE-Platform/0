// Platform-specific audio capture for Windows
// Uses cpal for microphone and WASAPI loopback for system audio

use crate::models::audio::{AudioDevice, AudioDeviceType, AudioError, AudioResult};
use cpal::traits::{DeviceTrait, HostTrait};

pub struct WindowsAudioCapture {
    device: Option<cpal::Device>,
    stream: Option<cpal::Stream>,
}

impl WindowsAudioCapture {
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

            // Get default config to extract sample rate and channels
            let default_config = device.default_input_config()
                .map_err(|e| AudioError::DeviceEnumerationError(format!("Failed to get device config: {}", e)))?;

            let is_default = default_input.as_ref()
                .and_then(|d| d.name().ok())
                .map(|n| n == name)
                .unwrap_or(false);

            devices.push(AudioDevice {
                id: name.clone(),
                name: name.clone(),
                device_type: AudioDeviceType::Microphone,
                is_default,
                sample_rate: default_config.sample_rate().0,
                channels: default_config.channels() as u32,
            });
        }

        // Add Windows-specific WASAPI loopback for system audio
        // Windows has native loopback support via WASAPI
        // The default output device can be opened in loopback mode
        if let Some(output_device) = host.default_output_device() {
            if let Ok(name) = output_device.name() {
                if let Ok(config) = output_device.default_output_config() {
                    devices.push(AudioDevice {
                        id: format!("{}_loopback", name),
                        name: format!("{} (WASAPI Loopback)", name),
                        device_type: AudioDeviceType::SystemLoopback,
                        is_default: false,
                        sample_rate: config.sample_rate().0,
                        channels: config.channels() as u32,
                    });
                }
            }
        }

        Ok(devices)
    }

    /// Start capturing from a device
    pub fn start_capture(&mut self, device_id: &str) -> AudioResult<()> {
        let host = cpal::default_host();

        // Check if this is a loopback device
        let is_loopback = device_id.contains("_loopback");

        let device = if device_id == "default" {
            host.default_input_device()
                .ok_or_else(|| AudioError::DeviceNotFound("No default input device".to_string()))?
        } else if is_loopback {
            // For loopback, use the output device
            // Remove the "_loopback" suffix to get the actual device name
            let actual_name = device_id.replace("_loopback", "");

            let devices = host.output_devices()
                .map_err(|e| AudioError::DeviceEnumerationError(format!("Failed to enumerate output devices: {}", e)))?;

            devices
                .filter(|d| d.name().map(|n| n == actual_name).unwrap_or(false))
                .next()
                .ok_or_else(|| AudioError::DeviceNotFound(format!("Loopback device not found: {}", actual_name)))?
        } else {
            // Search through input devices for matching name
            let devices = host.input_devices()
                .map_err(|e| AudioError::DeviceEnumerationError(format!("Failed to enumerate devices: {}", e)))?;

            devices
                .filter(|d| d.name().map(|n| n == device_id).unwrap_or(false))
                .next()
                .ok_or_else(|| AudioError::DeviceNotFound(format!("Device not found: {}", device_id)))?
        };

        let config = if is_loopback {
            device.default_output_config()
                .map_err(|e| AudioError::DeviceConfigError(format!("Failed to get loopback config: {}", e)))?
        } else {
            device.default_input_config()
                .map_err(|e| AudioError::DeviceConfigError(format!("Failed to get config: {}", e)))?
        };

        println!("Starting Windows audio capture for device: {} ({}Hz, {} channels, loopback: {})",
            device_id, config.sample_rate().0, config.channels(), is_loopback);

        self.device = Some(device);
        // Note: For WASAPI loopback on Windows, we need to use AUDCLNT_STREAMFLAGS_LOOPBACK
        // This will be implemented in the stream creation phase

        Ok(())
    }

    /// Stop capturing
    pub fn stop_capture(&mut self) -> AudioResult<()> {
        self.stream = None;
        self.device = None;
        println!("Stopped Windows audio capture");
        Ok(())
    }

    /// Get audio samples (non-blocking)
    pub fn read_samples(&mut self) -> AudioResult<Vec<f32>> {
        // TODO: Implement ring buffer for audio samples
        // This will be populated by the cpal stream callback
        Ok(vec![])
    }
}

// WASAPI loopback helper
pub fn get_default_loopback_device() -> AudioResult<String> {
    // TODO: Get default render device for loopback
    // Use IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender, eConsole)

    Ok("default_loopback".to_string())
}
