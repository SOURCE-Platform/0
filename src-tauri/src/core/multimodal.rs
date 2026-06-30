mod audio_capture;
mod constants;
mod indexing;
mod mappers;
mod media_io;
mod queries;
pub(crate) mod service;
mod types;
mod visual_capture;
mod visual_detections;

pub(crate) use media_io::{mediapipe_runtime_available, run_mediapipe_face_features};
pub use queries::{
    delete_all_multimodal_derived, get_asr_segments, get_audio_chunks, get_audio_state_spans,
    get_multimodal_activity_episode, get_visual_audio_summary, get_visual_scene_snapshot,
    get_visual_scene_snapshots, get_visual_state_spans,
};
pub use service::MultimodalService;
pub use types::{
    AsrSegmentDto, AudioChunkDto, AudioStateSpanDto, MultimodalActivityEpisodeDto,
    MultimodalCaptureOptions, MultimodalStartReport, RawDetectorOutputDto,
    VisionSceneDetailPayload, VisualAudioSummaryDto, VisualSceneSnapshotDto, VisualStateSpanDto,
};

type MultimodalQueryResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
