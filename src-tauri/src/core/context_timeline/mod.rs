use crate::core::database::Database;
use crate::core::gaze;
use crate::core::multimodal::{
    self, AsrSegmentDto, AudioStateSpanDto, SoundEventDetectionDto, SoundEventSpanDto,
    SpeechEmotionSegmentDto, VisualSceneSnapshotDto, VisualStateSpanDto, DICTATION_SOURCE_ID,
};
use crate::core::ocr_agent_context::{self, AgentSceneSnapshotDto};
use crate::models::activity::AppInfo;
use image::GenericImageView;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_BUCKET_MS: i64 = 30_000;
const DEFAULT_SNAPSHOT_SPAN_MS: i64 = 5_000;
pub const DEFAULT_VISIBLE_WINDOW_MS: i64 = 15 * 60 * 1000;

include!("types.rs");
include!("waveform_types.rs");
include!("schema.rs");
include!("storage.rs");
include!("pii.rs");
include!("timeline.rs");
include!("inspector.rs");
include!("detail.rs");
include!("detail_payloads_context.rs");
include!("detail_payloads_media.rs");
include!("detail_payloads_attention.rs");
include!("reviews.rs");
include!("rails_system.rs");
include!("rails_activity.rs");
include!("rails_summary.rs");
include!("rails_media.rs");
include!("rails_audio_labels.rs");
include!("rails_audio_waveforms.rs");
include!("rails_audio_emotions.rs");
include!("rails_audio_intelligence.rs");
include!("rails_attention.rs");
include!("fetch.rs");
