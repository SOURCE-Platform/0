use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use hound::{WavSpec, WavWriter};
use std::path::Path;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::time::{Duration, Instant};

const TARGET_SAMPLE_RATE: u32 = 16_000;

struct CaptureConfig {
    rate: u32,
    channels: usize,
}

pub(crate) async fn capture_microphone_chunk(
    source_name: &str,
    duration_secs: f32,
    output_path: &Path,
) -> Result<(), String> {
    let source_name = source_name.to_string();
    let output_path = output_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        capture_chunk_blocking(&source_name, duration_secs, &output_path)
    })
    .await
    .map_err(|error| format!("Microphone capture task failed: {error}"))?
}

fn capture_chunk_blocking(
    source_name: &str,
    duration_secs: f32,
    output_path: &Path,
) -> Result<(), String> {
    let host = cpal::default_host();
    let device = find_input_device(&host, source_name)?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("Could not read microphone format: {e}"))?;
    let capture = CaptureConfig {
        rate: config.sample_rate().0,
        channels: config.channels() as usize,
    };
    let target_samples = (duration_secs * capture.rate as f32) as usize * capture.channels;

    let (tx, rx) = sync_channel::<Vec<f32>>(64);
    let stream = build_capture_stream(&device, &config, tx)?;
    stream
        .play()
        .map_err(|e| format!("Could not start microphone capture stream: {e}"))?;

    let samples = collect_samples(&rx, target_samples, duration_secs);
    drop(stream);

    let samples = samples?;
    write_wav(&capture, &samples, output_path)
}

fn collect_samples(
    rx: &std::sync::mpsc::Receiver<Vec<f32>>,
    target_samples: usize,
    duration_secs: f32,
) -> Result<Vec<f32>, String> {
    let mut samples = Vec::with_capacity(target_samples);
    let deadline = Instant::now() + Duration::from_secs_f32(duration_secs * 2.0 + 3.0);
    while samples.len() < target_samples {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Microphone capture did not deliver enough samples in time.".to_string());
        }
        match rx.recv_timeout(remaining) {
            Ok(batch) => samples.extend_from_slice(&batch),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return Err("Microphone capture stalled while collecting samples.".to_string());
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(samples)
}

fn write_wav(config: &CaptureConfig, samples: &[f32], output_path: &Path) -> Result<(), String> {
    let mono = downmix_to_mono(samples, config.channels);
    let resampled = resample_linear(&mono, config.rate, TARGET_SAMPLE_RATE);
    let spec = WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = WavWriter::create(output_path, spec)
        .map_err(|e| format!("Could not create microphone chunk file: {e}"))?;
    for sample in resampled {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(value)
            .map_err(|e| format!("Could not write microphone chunk: {e}"))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("Could not finalize microphone chunk: {e}"))
}

fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.is_empty() || from_rate == to_rate {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = ((samples.len() as f64) / ratio).max(1.0) as usize;
    let mut out = Vec::with_capacity(output_len);
    for index in 0..output_len {
        let position = index as f64 * ratio;
        let left = position.floor() as usize;
        let right = (left + 1).min(samples.len() - 1);
        let fraction = (position - left as f64) as f32;
        out.push(samples[left] * (1.0 - fraction) + samples[right] * fraction);
    }
    out
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

fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn build_capture_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    tx: SyncSender<Vec<f32>>,
) -> Result<Stream, String> {
    let stream_config = config.config();
    let err_fn = |error| eprintln!("microphone capture stream error: {error}");
    let format = config.sample_format();
    let build = match format {
        SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| send_batch(&tx, data.to_vec()),
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                send_batch(
                    &tx,
                    data.iter()
                        .map(|sample| *sample as f32 / i16::MAX as f32)
                        .collect(),
                )
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                send_batch(
                    &tx,
                    data.iter()
                        .map(|sample| (*sample as f32 - 32768.0) / 32768.0)
                        .collect(),
                )
            },
            err_fn,
            None,
        ),
        other => {
            return Err(format!(
                "Unsupported microphone capture sample format: {other:?}"
            ))
        }
    };
    build.map_err(|e| format!("Could not build microphone capture stream ({format:?}): {e}"))
}

fn send_batch(tx: &SyncSender<Vec<f32>>, batch: Vec<f32>) {
    if batch.is_empty() {
        return;
    }
    let _ = tx.try_send(batch);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_stereo_frames() {
        let stereo = [1.0, -1.0, 0.5, 0.25];
        let mono = downmix_to_mono(&stereo, 2);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.0).abs() < 1e-6);
        assert!((mono[1] - 0.375).abs() < 1e-6);
    }

    #[test]
    fn resample_halves_48k_to_16k_length() {
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 / 100.0).sin()).collect();
        let output = resample_linear(&input, 48_000, 16_000);
        assert!((output.len() as i64 - 1600).abs() <= 1);
    }

    #[test]
    fn resample_passthrough_when_rates_match() {
        let input = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }
}
