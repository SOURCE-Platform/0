mod audio_capture;
mod audio_capture_support;
mod audio_intelligence_indexing;
mod audio_intelligence_queries;
mod audio_intelligence_types;
mod audio_meter;
mod audio_meter_types;
mod audio_runtime;
mod audio_sources;
mod audio_transcripts;
mod bench_wer;
mod constants;
mod desktop_audio_runtime;
pub(crate) mod dictation_helper;
mod dictation_pipeline;
pub(crate) mod dictation_supervisor;
mod foreground_coordinator;
mod speech_model;
mod indexing;
mod mappers;
mod media_io;
mod parakeet_worker;
mod queries;
mod retention;
pub(crate) mod service;
pub(crate) mod speech_provider;
mod types;
mod visual_capture;
mod visual_detections;
mod visual_loop_state;

pub use audio_intelligence_queries::{
    get_sound_event_detections, get_sound_event_spans, get_speech_emotion_segments,
};
pub use audio_intelligence_types::{
    SoundEventDetectionDto, SoundEventSpanDto, SpeechEmotionSegmentDto,
};
pub(crate) use audio_meter::{current_audio_meters, prepare_audio_meters, sample_audio_meters};
pub use audio_meter_types::{AudioMeterReadingDto, AudioMetersDto};
pub(crate) use audio_sources::{
    choose_video_source, default_audio_input_name, list_avfoundation_sources,
};
pub(crate) use dictation_helper::DictationHelper;
pub(crate) use dictation_pipeline::PipelineAction;
pub(crate) use dictation_supervisor::{DictationSupervisor, SupervisorCommand};
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
