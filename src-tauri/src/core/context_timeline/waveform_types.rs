/// A compact, continuous envelope for a single captured audio source. Raw
/// chunks remain in SQLite; this is only the stitched timeline presentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineWaveformDto {
    pub source_id: String,
    pub source_label: String,
    pub samples: Vec<TimelineWaveformSampleDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineWaveformSampleDto {
    pub timestamp: i64,
    pub level: f32,
}
