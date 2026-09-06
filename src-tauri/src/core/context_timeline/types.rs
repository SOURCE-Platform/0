#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSnapshotDto {
    pub app_name: String,
    pub bundle_id: String,
    pub process_id: u32,
    pub is_frontmost: bool,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSlice {
    pub id: String,
    pub rail: String,
    pub slice_kind: String,
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub source: String,
    pub confidence: f32,
    pub session_id: Option<String>,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub interaction_state: Option<String>,
    pub reasons: Vec<String>,
    pub visible_windows: Vec<WindowSnapshotDto>,
    pub ocr_preview: Option<String>,
    pub pii_count: usize,
    pub evidence_frame_path: Option<String>,
    pub storage_bytes: u64,
    pub storage_exact: bool,
    pub row_count: u64,
    pub file_count: u64,
    pub has_detail_view: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRailDto {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub description: String,
    pub confidence_note: String,
    pub default_expanded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waveform: Option<TimelineWaveformDto>,
    pub slices: Vec<ContextSlice>,
    pub children: Vec<TimelineRailDto>,
}

impl TimelineRailDto {
    fn lane(
        id: &str,
        label: &str,
        description: &str,
        confidence_note: &str,
        slices: Vec<ContextSlice>,
    ) -> Self {
        Self {
            id: id.to_string(),
            kind: "lane".to_string(),
            label: label.to_string(),
            description: description.to_string(),
            confidence_note: confidence_note.to_string(),
            default_expanded: true,
            waveform: None,
            slices,
            children: Vec::new(),
        }
    }

    fn group(
        id: &str,
        label: &str,
        description: &str,
        confidence_note: &str,
        default_expanded: bool,
        children: Vec<TimelineRailDto>,
    ) -> Self {
        Self {
            id: id.to_string(),
            kind: "group".to_string(),
            label: label.to_string(),
            description: description.to_string(),
            confidence_note: confidence_note.to_string(),
            default_expanded,
            waveform: None,
            slices: Vec::new(),
            children,
        }
    }

    fn lane_with_waveform(
        id: &str,
        label: &str,
        description: &str,
        confidence_note: &str,
        waveform: TimelineWaveformDto,
        slices: Vec<ContextSlice>,
    ) -> Self {
        let mut rail = Self::lane(id, label, description, confidence_note, slices);
        rail.waveform = Some(waveform);
        rail
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InteractionSummaryDto {
    pub active_typing_ms: i64,
    pub active_pointer_ms: i64,
    pub passive_viewing_ms: i64,
    pub voice_input_inferred_ms: i64,
    pub mixed_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppMetricSummaryDto {
    pub focused_app_count: usize,
    pub visible_app_count: usize,
    pub total_focus_time_ms: i64,
    pub total_visible_time_ms: i64,
    pub total_interaction_time_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSummaryDto {
    pub ocr_block_count: usize,
    pub evidence_frame_count: usize,
    pub interaction: InteractionSummaryDto,
    pub app_metrics: AppMetricSummaryDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextTimelineData {
    pub start_timestamp: i64,
    pub end_timestamp: i64,
    pub now_timestamp: i64,
    pub default_visible_window_ms: i64,
    pub summary: TimelineSummaryDto,
    pub rails: Vec<TimelineRailDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawPayloadDto {
    pub label: String,
    pub raw_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrReconstructionBlockDto {
    pub id: String,
    pub text: String,
    pub confidence: f32,
    pub bounding_box: Option<serde_json::Value>,
    pub pii_entities: Vec<PiiEntityDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrReconstructionDto {
    pub width: u32,
    pub height: u32,
    pub frame_path: Option<String>,
    pub backdrop_available: bool,
    pub blocks: Vec<OcrReconstructionBlockDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSliceDetailDto {
    pub slice: ContextSlice,
    pub rail_label: String,
    pub occurred_at: i64,
    pub duration_ms: i64,
    pub storage_bytes: u64,
    pub storage_exact: bool,
    pub row_count: u64,
    pub file_count: u64,
    pub linked_file_paths: Vec<String>,
    pub focused_app: Option<String>,
    pub focused_bundle_id: Option<String>,
    pub visible_windows: Vec<WindowSnapshotDto>,
    pub interaction_reasons: Vec<String>,
    pub pii_entities: Vec<PiiEntityDto>,
    pub raw_payloads: Vec<RawPayloadDto>,
    pub ocr_reconstruction: Option<OcrReconstructionDto>,
    pub nearby_system_events: Vec<ContextSlice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInspectorDto {
    pub timestamp: i64,
    pub focused_app: Option<String>,
    pub focused_bundle_id: Option<String>,
    pub visible_windows: Vec<WindowSnapshotDto>,
    pub interaction_state: Option<String>,
    pub interaction_reasons: Vec<String>,
    pub recent_input_state: String,
    pub ocr_text: Vec<String>,
    pub pii_entities: Vec<PiiEntityDto>,
    pub evidence_frame_path: Option<String>,
    pub nearby_system_events: Vec<ContextSlice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageOverviewItemDto {
    pub app_name: String,
    pub bundle_id: String,
    pub focused_time_ms: i64,
    pub visible_time_ms: i64,
    pub interaction_time_ms: i64,
    pub ocr_hit_count: usize,
    pub recent_segment_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageOverviewDto {
    pub items: Vec<AppUsageOverviewItemDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiiEntityDto {
    pub id: String,
    pub timestamp: i64,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub entity_type: String,
    pub redacted_preview: String,
    pub confidence: f32,
    pub context_text: String,
    pub bounding_box: Option<serde_json::Value>,
    pub frame_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrReviewItemDto {
    pub id: String,
    pub timestamp: i64,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub text: String,
    pub confidence: f32,
    pub frame_path: Option<String>,
    pub pii_entities: Vec<PiiEntityDto>,
}

#[derive(Debug, Clone, FromRow)]
struct ContextEventRow {
    id: String,
    session_id: Option<String>,
    timestamp: i64,
    channel: String,
    event_type: String,
    source: String,
    confidence: f64,
    payload_json: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct WindowSnapshotRow {
    id: String,
    session_id: Option<String>,
    timestamp: i64,
    frontmost_app_name: Option<String>,
    frontmost_bundle_id: Option<String>,
    visible_windows_json: String,
    confidence: f64,
    source: String,
}

#[derive(Debug, Clone, FromRow)]
struct KeyboardEventSummaryRow {
    timestamp: i64,
    app_name: String,
    window_title: String,
}

#[derive(Debug, Clone, FromRow)]
struct MouseEventSummaryRow {
    timestamp: i64,
    app_name: String,
    window_title: String,
}

#[derive(Debug, Clone, FromRow)]
struct OcrRow {
    id: String,
    session_id: String,
    timestamp: i64,
    frame_path: Option<String>,
    text: String,
    confidence: f64,
    bounding_box: String,
}

#[derive(Debug, Clone, FromRow)]
struct FrameRow {
    session_id: String,
    timestamp: i64,
    file_path: String,
}

#[derive(Debug, Clone, FromRow)]
struct SessionRow {
    id: String,
    start_timestamp: i64,
    end_timestamp: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct AppUsageRow {
    session_id: String,
    app_name: String,
    bundle_id: String,
    process_id: i64,
    start_timestamp: i64,
    end_timestamp: Option<i64>,
}

#[derive(Debug, Clone)]
struct OcrEventGroup {
    id: String,
    session_id: String,
    timestamp: i64,
    frame_path: Option<String>,
    blocks: Vec<OcrRow>,
}

#[derive(Debug, Clone)]
struct InferredAppContext {
    app_name: Option<String>,
    bundle_id: Option<String>,
    window_title: Option<String>,
}
