use super::audio_meter_types::{AudioMeterReadingDto, AudioMetersDto};
use super::audio_sources::{
    choose_audio_source, default_audio_input_name, list_avfoundation_sources,
};
use super::desktop_audio_runtime::{current_live_desktop_meter, ensure_live_desktop_meter};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, Stream, StreamConfig};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
static LIVE_METER: OnceLock<Mutex<Option<LiveMeterHandle>>> = OnceLock::new();
struct LiveMeterHandle {
    source_key: String,
    source_name: String,
    level_bits: Arc<AtomicU32>,
    stop_tx: Sender<()>,
    _thread: JoinHandle<()>,
}
impl Drop for LiveMeterHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}
pub(crate) async fn sample_audio_meters(
    preferred_source_id: Option<&str>,
    desktop_enabled: bool,
    desktop_gain_db: f32,
) -> Result<AudioMetersDto, String> {
    let microphone = sample_microphone_meter(preferred_source_id).await;
    Ok(AudioMetersDto {
        microphone,
        desktop: prepare_desktop_meter(desktop_enabled, desktop_gain_db),
    })
}
/// Resolves the selected source once, then leaves the live stream to feed the UI.
pub(crate) async fn prepare_audio_meters(
    preferred_source_id: Option<&str>,
    desktop_enabled: bool,
    desktop_gain_db: f32,
) -> Result<AudioMetersDto, String> {
    sample_audio_meters(preferred_source_id, desktop_enabled, desktop_gain_db).await
}
/// Returns the most recent callback value without rediscovering macOS devices.
pub(crate) fn current_audio_meters(desktop_enabled: bool, desktop_gain_db: f32) -> AudioMetersDto {
    let microphone = LIVE_METER
        .get()
        .and_then(|meter| meter.lock().ok())
        .and_then(|meter| {
            meter.as_ref().map(|active| {
                AudioMeterReadingDto::active(
                    active.source_key.clone(),
                    active.source_name.clone(),
                    read_level(&active.level_bits),
                )
            })
        })
        .unwrap_or_else(|| {
            AudioMeterReadingDto::unavailable(
                "microphone:unknown",
                "Microphone",
                "Choose a microphone to start live metering.",
            )
        });

    AudioMetersDto {
        microphone,
        desktop: current_desktop_meter(desktop_enabled, desktop_gain_db),
    }
}
fn prepare_desktop_meter(enabled: bool, gain_db: f32) -> AudioMeterReadingDto {
    if !enabled {
        return AudioMeterReadingDto::unavailable(
            "desktop_output:system",
            "Desktop audio",
            "Desktop audio is turned off.",
        );
    }
    match ensure_live_desktop_meter(gain_db) {
        Ok(level) => AudioMeterReadingDto::active(
            "desktop_output:system".to_string(),
            "Desktop audio".to_string(),
            desktop_display_level(level, gain_db),
        ),
        Err(error) => {
            AudioMeterReadingDto::unavailable("desktop_output:system", "Desktop audio", error)
        }
    }
}

fn current_desktop_meter(enabled: bool, gain_db: f32) -> AudioMeterReadingDto {
    if !enabled {
        return prepare_desktop_meter(false, gain_db);
    }
    match current_live_desktop_meter(gain_db) {
        Ok(level) => AudioMeterReadingDto::active(
            "desktop_output:system".to_string(),
            "Desktop audio".to_string(),
            desktop_display_level(level, gain_db),
        ),
        Err(error) => {
            AudioMeterReadingDto::unavailable("desktop_output:system", "Desktop audio", error)
        }
    }
}

async fn sample_microphone_meter(preferred_source_id: Option<&str>) -> AudioMeterReadingDto {
    let sources = match list_avfoundation_sources().await {
        Ok(sources) => sources,
        Err(error) => {
            return AudioMeterReadingDto::unavailable("microphone:unknown", "Microphone", error)
        }
    };
    let default_source_name = default_audio_input_name().await;
    let Some(source) = choose_audio_source(
        &sources.audio,
        preferred_source_id,
        default_source_name.as_deref(),
    ) else {
        return AudioMeterReadingDto::unavailable(
            "microphone:unknown",
            "Microphone",
            "No microphone input is available.",
        );
    };

    let source_id = format!("microphone:{}", source.index);
    match ensure_live_meter(&source_id, &source.name) {
        Ok(level) => AudioMeterReadingDto::active(source_id, source.name, level),
        Err(error) => AudioMeterReadingDto::unavailable(source_id, source.name, error),
    }
}

fn ensure_live_meter(source_id: &str, source_name: &str) -> Result<f32, String> {
    let meter_ref = LIVE_METER.get_or_init(|| Mutex::new(None));
    let mut meter = meter_ref
        .lock()
        .map_err(|_| "Microphone meter lock is unavailable.".to_string())?;

    if !matches!(meter.as_ref(), Some(active) if active.source_key == source_id) {
        *meter = Some(start_live_meter(source_id, source_name)?);
    }

    let Some(active) = meter.as_ref() else {
        return Err("Microphone meter did not start.".to_string());
    };
    Ok(read_level(&active.level_bits))
}

fn start_live_meter(source_id: &str, source_name: &str) -> Result<LiveMeterHandle, String> {
    let level_bits = Arc::new(AtomicU32::new(0f32.to_bits()));
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let worker_level = Arc::clone(&level_bits);
    let worker_source_name = source_name.to_string();

    let handle = thread::spawn(move || {
        start_meter_thread(worker_source_name, worker_level, stop_rx, ready_tx);
    });

    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(LiveMeterHandle {
            source_key: source_id.to_string(),
            source_name: source_name.to_string(),
            level_bits,
            stop_tx,
            _thread: handle,
        }),
        Ok(Err(error)) => {
            let _ = stop_tx.send(());
            Err(error)
        }
        Err(_) => {
            let _ = stop_tx.send(());
            Err("Microphone meter did not become ready quickly enough.".to_string())
        }
    }
}

fn create_meter_stream(source_name: &str, level_bits: Arc<AtomicU32>) -> Result<Stream, String> {
    let host = cpal::default_host();
    let device = find_input_device(&host, source_name)?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("Could not read microphone format: {e}"))?;
    let stream = build_low_latency_stream(&device, &config, level_bits)?;
    stream
        .play()
        .map_err(|e| format!("Could not start microphone meter stream: {e}"))?;
    Ok(stream)
}

fn run_meter_stream(stream: Stream, stop_rx: mpsc::Receiver<()>) -> Result<(), String> {
    let _stream = stream;
    let _ = stop_rx.recv();
    Ok(())
}

fn notify_ready(ready_tx: &mpsc::Sender<Result<(), String>>) {
    let _ = ready_tx.send(Ok(()));
}

fn start_meter_thread(
    source_name: String,
    level_bits: Arc<AtomicU32>,
    stop_rx: mpsc::Receiver<()>,
    ready_tx: mpsc::Sender<Result<(), String>>,
) {
    let result = create_meter_stream(&source_name, level_bits).map(|stream| {
        notify_ready(&ready_tx);
        let _ = run_meter_stream(stream, stop_rx);
    });

    if let Err(error) = result {
        let _ = ready_tx.send(Err(error));
    }
}

fn find_input_device(host: &cpal::Host, source_name: &str) -> Result<cpal::Device, String> {
    let target = normalize_name(source_name);
    let devices = host
        .input_devices()
        .map_err(|e| format!("Could not list microphone devices: {e}"))?;
    for device in devices {
        let name = device.name().unwrap_or_default();
        if normalize_name(&name) == target {
            return Ok(device);
        }
    }
    host.default_input_device()
        .ok_or_else(|| "No default microphone device is available.".to_string())
}

fn build_low_latency_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    level_bits: Arc<AtomicU32>,
) -> Result<Stream, String> {
    let default_config = config.config();
    let low_latency_config = StreamConfig {
        channels: default_config.channels,
        sample_rate: default_config.sample_rate,
        // 256 frames is roughly 5 ms at 48 kHz. If a device rejects it,
        // fall back to its system-selected buffer instead of failing metering.
        buffer_size: BufferSize::Fixed(256),
    };
    build_stream_with_config(device, config, low_latency_config, Arc::clone(&level_bits)).or_else(
        |low_latency_error| {
            build_stream_with_config(device, config, default_config, level_bits).map_err(
                |default_error| {
                    format!(
                        "Could not build low-latency microphone meter ({low_latency_error}); \
                         default buffer also failed: {default_error}"
                    )
                },
            )
        },
    )
}

fn build_stream_with_config(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    stream_config: StreamConfig,
    level_bits: Arc<AtomicU32>,
) -> Result<Stream, String> {
    let err_fn = |error| eprintln!("microphone meter stream error: {error}");
    match config.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| update_level(data.iter().copied(), &level_bits),
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                update_level(
                    data.iter().map(|sample| *sample as f32 / i16::MAX as f32),
                    &level_bits,
                )
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                update_level(
                    data.iter()
                        .map(|sample| (*sample as f32 - 32768.0) / 32768.0),
                    &level_bits,
                )
            },
            err_fn,
            None,
        ),
        other => {
            return Err(format!(
                "Unsupported microphone meter sample format: {other:?}"
            ));
        }
    }
    .map_err(|e| format!("Could not build microphone meter stream: {e}"))
}

fn update_level(samples: impl Iterator<Item = f32>, level_bits: &AtomicU32) {
    let mut total = 0.0f32;
    let mut count = 0usize;
    for sample in samples {
        total += sample * sample;
        count += 1;
    }
    if count == 0 {
        return;
    }
    let rms = (total / count as f32).sqrt();
    level_bits.store(meter_level(rms).to_bits(), Ordering::Relaxed);
}

fn read_level(level_bits: &AtomicU32) -> f32 {
    f32::from_bits(level_bits.load(Ordering::Relaxed)).clamp(0.0, 1.0)
}

fn normalize_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn meter_level(rms: f32) -> f32 {
    // Speech RMS values are small; this curve keeps quiet speech visible.
    (rms * 16.0).sqrt().clamp(0.0, 1.0)
}

fn desktop_display_level(rms: f32, gain_db: f32) -> f32 {
    let gain = 10f32.powf(gain_db.clamp(0.0, 24.0) / 20.0);
    (rms * gain * 0.75).sqrt().clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::meter_level;

    #[test]
    fn meter_curve_keeps_quiet_speech_visible() {
        assert!(meter_level(0.01) > 0.35);
        assert_eq!(meter_level(1.0), 1.0);
    }
}
