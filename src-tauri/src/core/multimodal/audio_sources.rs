use super::types::{AvFoundationSource, AvFoundationSources};
use regex::Regex;
use serde_json::Value;
use tokio::process::Command;

pub(crate) async fn list_avfoundation_sources() -> Result<AvFoundationSources, String> {
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-f",
            "avfoundation",
            "-list_devices",
            "true",
            "-i",
            "",
        ])
        .output()
        .await
        .map_err(|error| format!("Failed to inspect AVFoundation devices: {error}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let line_re = Regex::new(r"\[(\d+)\]\s+(.+)$").map_err(|error| error.to_string())?;
    let mut current = "";
    let mut sources = AvFoundationSources::default();

    for line in text.lines() {
        if line.contains("AVFoundation video devices") {
            current = "video";
            continue;
        }
        if line.contains("AVFoundation audio devices") {
            current = "audio";
            continue;
        }
        if let Some(captures) = line_re.captures(line) {
            let index = captures
                .get(1)
                .and_then(|value| value.as_str().parse::<i32>().ok())
                .unwrap_or(-1);
            let name = captures
                .get(2)
                .map(|value| value.as_str().trim().to_string())
                .unwrap_or_default();
            let source = AvFoundationSource { index, name };
            match current {
                "video" => sources.video.push(source),
                "audio" => sources.audio.push(source),
                _ => {}
            }
        }
    }
    Ok(sources)
}

pub(crate) async fn default_audio_input_name() -> Option<String> {
    let output = Command::new("system_profiler")
        .args(["SPAudioDataType", "-json"])
        .output()
        .await
        .ok()?;
    let profile: Value = serde_json::from_slice(&output.stdout).ok()?;
    let categories = profile.get("SPAudioDataType")?.as_array()?;

    categories
        .iter()
        .flat_map(|category| {
            category
                .get("_items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find(|item| {
            item.get("coreaudio_default_audio_input_device")
                .and_then(Value::as_str)
                == Some("spaudio_yes")
        })
        .and_then(|item| item.get("_name").and_then(Value::as_str))
        .map(str::to_string)
}

pub(crate) fn choose_video_source(sources: &[AvFoundationSource]) -> Option<AvFoundationSource> {
    sources
        .iter()
        .find(|source| source.name.contains("FaceTime"))
        .or_else(|| {
            sources.iter().find(|source| {
                let lower = source.name.to_lowercase();
                !lower.contains("capture screen") && !lower.contains("desk view")
            })
        })
        .or_else(|| sources.first())
        .cloned()
}

pub(crate) fn choose_audio_source(
    sources: &[AvFoundationSource],
    preferred_source_id: Option<&str>,
    default_source_name: Option<&str>,
) -> Option<AvFoundationSource> {
    if let Some(source_id) = preferred_source_id {
        if let Some(index) = source_id
            .strip_prefix("microphone:")
            .and_then(|value| value.parse::<i32>().ok())
        {
            if let Some(source) = sources.iter().find(|source| source.index == index) {
                return Some(source.clone());
            }
        }
    }

    default_source_name
        .and_then(|name| sources.iter().find(|source| source.name == name))
        .or_else(|| {
            sources
                .iter()
                .find(|source| source.name.contains("MacBook Air Microphone"))
        })
        .or_else(|| {
            sources
                .iter()
                .find(|source| source.name.contains("Microphone"))
        })
        .or_else(|| sources.first())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_selection_prefers_the_macos_default_input() {
        let sources = vec![
            AvFoundationSource {
                index: 0,
                name: "MacBook Air Microphone".to_string(),
            },
            AvFoundationSource {
                index: 1,
                name: "USB-C to 3.5mm Headphone Jack Adapter".to_string(),
            },
        ];

        let selected = choose_audio_source(
            &sources,
            None,
            Some("USB-C to 3.5mm Headphone Jack Adapter"),
        )
        .expect("a default source should be selected");

        assert_eq!(selected.index, 1);
    }
}
