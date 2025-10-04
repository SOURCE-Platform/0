// OCR processing pipeline with job queue and worker

use crate::core::ocr_engine::{OcrEngine, OcrError};
use crate::core::ocr_storage::{OcrStorage, ProcessedOcrResult};
use crate::models::capture::{PixelFormat, RawFrame};
use crate::models::ocr::{BoundingBox, OcrResult};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

// ==============================================================================
// Configuration
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrProcessorConfig {
    pub enabled: bool,
    pub interval_seconds: u32,     // How often to run OCR (default: 60)
    pub batch_size: usize,         // Frames to process at once (default: 5)
    pub skip_static_frames: bool,  // Only OCR frames with motion (default: true)
    pub max_queue_size: usize,     // Max pending jobs (default: 100)
}

impl Default for OcrProcessorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: 60,
            batch_size: 5,
            skip_static_frames: true,
            max_queue_size: 100,
        }
    }
}

// ==============================================================================
// OCR Job
// ==============================================================================

#[derive(Debug, Clone)]
pub struct OcrJob {
    pub session_id: Uuid,
    pub frame_path: PathBuf,
    pub timestamp: i64,
    pub motion_regions: Vec<BoundingBox>,
}

// ==============================================================================
// OCR Processor
// ==============================================================================

pub struct OcrProcessor {
    ocr_engine: Arc<OcrEngine>,
    storage: Arc<OcrStorage>,
    processing_queue: Arc<RwLock<VecDeque<OcrJob>>>,
    is_processing: Arc<RwLock<bool>>,
    config: OcrProcessorConfig,
    metrics: Arc<RwLock<OcrMetrics>>,
}

impl OcrProcessor {
    pub fn new(
        ocr_engine: Arc<OcrEngine>,
        storage: Arc<OcrStorage>,
        config: OcrProcessorConfig,
    ) -> Self {
        Self {
            ocr_engine,
            storage,
            processing_queue: Arc::new(RwLock::new(VecDeque::new())),
            is_processing: Arc::new(RwLock::new(false)),
            config,
            metrics: Arc::new(RwLock::new(OcrMetrics::default())),
        }
    }

    /// Start the OCR processor worker
    pub async fn start(&self) -> Result<(), OcrError> {
        if !self.config.enabled {
            return Ok(());
        }

        let mut is_processing = self.is_processing.write().await;
        if *is_processing {
            return Ok(()); // Already running
        }
        *is_processing = true;
        drop(is_processing);

        // Spawn processing worker
        let queue = self.processing_queue.clone();
        let ocr_engine = self.ocr_engine.clone();
        let storage = self.storage.clone();
        let batch_size = self.config.batch_size;
        let is_processing_flag = self.is_processing.clone();
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            while *is_processing_flag.read().await {
                let jobs = {
                    let mut queue = queue.write().await;
                    let count = queue.len().min(batch_size);
                    queue.drain(..count).collect::<Vec<_>>()
                };

                if jobs.is_empty() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }

                // Process batch
                for job in jobs {
                    match Self::process_job(&ocr_engine, job).await {
                        Ok(result) => {
                            // Update metrics
                            {
                                let mut m = metrics.write().await;
                                m.frames_processed += 1;
                                m.text_blocks_extracted += result.ocr_result.text_blocks.len() as u64;
                                m.total_processing_time_ms += result.ocr_result.processing_time_ms;
                            }

                            // Save to database
                            if let Err(e) = storage.save_ocr_result(result).await {
                                eprintln!("Failed to save OCR result: {}", e);
                            }
                        }
                        Err(e) => {
                            eprintln!("OCR processing error: {}", e);
                        }
                    }
                }

                // Small delay between batches
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });

        Ok(())
    }

    /// Enqueue a frame for OCR processing
    pub async fn enqueue_frame(
        &self,
        session_id: Uuid,
        frame_path: PathBuf,
        timestamp: i64,
        motion_regions: Vec<BoundingBox>,
    ) -> Result<(), OcrError> {
        if !self.config.enabled {
            return Ok(());
        }

        // Skip static frames if configured
        if self.config.skip_static_frames && motion_regions.is_empty() {
            return Ok(());
        }

        let mut queue = self.processing_queue.write().await;

        // Check queue size limit
        if queue.len() >= self.config.max_queue_size {
            // Drop oldest job
            queue.pop_front();
        }

        queue.push_back(OcrJob {
            session_id,
            frame_path,
            timestamp,
            motion_regions,
        });

        // Update queue size metric
        {
            let mut metrics = self.metrics.write().await;
            metrics.queue_size = queue.len();
        }

        Ok(())
    }

    /// Process a single OCR job
    async fn process_job(
        ocr_engine: &Arc<OcrEngine>,
        job: OcrJob,
    ) -> Result<ProcessedOcrResult, OcrError> {
        // Load frame from disk
        let frame = Self::load_frame(&job.frame_path)?;

        // Decide whether to OCR full frame or just motion regions
        let ocr_result = if !job.motion_regions.is_empty() && Self::should_use_regions(&job.motion_regions) {
            // OCR only motion regions (more efficient)
            let mut results = Vec::new();

            for region in &job.motion_regions {
                let result = ocr_engine.extract_text_from_region(&frame, region).await?;
                results.push(result);
            }

            Self::merge_ocr_results(results)
        } else {
            // OCR full frame
            ocr_engine.extract_text_from_frame(&frame).await?
        };

        Ok(ProcessedOcrResult {
            session_id: job.session_id,
            timestamp: job.timestamp,
            frame_path: Some(job.frame_path),
            ocr_result,
        })
    }

    /// Load frame from disk
    fn load_frame(path: &PathBuf) -> Result<RawFrame, OcrError> {
        let img = image::open(path)
            .map_err(|e| OcrError::Processing(format!("Failed to load image: {}", e)))?;
        let rgba = img.to_rgba8();

        Ok(RawFrame {
            timestamp: 0, // Not used in OCR
            width: rgba.width(),
            height: rgba.height(),
            data: rgba.into_raw(),
            format: PixelFormat::RGBA8,
        })
    }

    /// Determine if we should use region-based OCR
    fn should_use_regions(regions: &[BoundingBox]) -> bool {
        // Use region-based OCR if we have a reasonable number of small regions
        // Skip if we have too many regions (likely noisy) or one very large region (likely video)
        if regions.is_empty() || regions.len() > 10 {
            return false;
        }

        // Check if any region is too large (> 50% of 1080p screen)
        let max_reasonable_area = (1920 * 1080) / 2;
        for region in regions {
            if region.area() > max_reasonable_area {
                return false; // Likely video playback
            }
        }

        true
    }

    /// Merge multiple OCR results into one
    fn merge_ocr_results(results: Vec<OcrResult>) -> OcrResult {
        let mut all_blocks = Vec::new();
        let mut all_text = Vec::new();
        let total_time: u64 = results.iter().map(|r| r.processing_time_ms).sum();

        for result in results {
            all_blocks.extend(result.text_blocks);
            if !result.total_text.is_empty() {
                all_text.push(result.total_text);
            }
        }

        let mut merged = OcrResult::new(
            chrono::Utc::now().timestamp_millis(),
            all_blocks,
            total_time,
        );
        merged.total_text = all_text.join(" ");
        merged
    }

    /// Stop the OCR processor
    pub async fn stop(&self) -> Result<(), OcrError> {
        *self.is_processing.write().await = false;

        // Wait for queue to empty (with timeout)
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(30);

        while start.elapsed() < timeout {
            let queue_len = self.processing_queue.read().await.len();
            if queue_len == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    /// Get current metrics
    pub async fn get_metrics(&self) -> OcrMetrics {
        let metrics = self.metrics.read().await;
        let queue_size = self.processing_queue.read().await.len();

        let mut result = metrics.clone();
        result.queue_size = queue_size;

        // Calculate average processing time
        if result.frames_processed > 0 {
            result.average_processing_time_ms =
                result.total_processing_time_ms as f64 / result.frames_processed as f64;
        }

        result
    }

    /// Reset metrics
    pub async fn reset_metrics(&self) {
        *self.metrics.write().await = OcrMetrics::default();
    }

    /// Get configuration
    pub fn config(&self) -> &OcrProcessorConfig {
        &self.config
    }

    /// Check if processor is running
    pub async fn is_running(&self) -> bool {
        *self.is_processing.read().await
    }
}

// ==============================================================================
// Metrics
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OcrMetrics {
    pub frames_processed: u64,
    pub text_blocks_extracted: u64,
    pub total_processing_time_ms: u64,
    pub average_processing_time_ms: f64,
    pub queue_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_processor_config_default() {
        let config = OcrProcessorConfig::default();
        assert_eq!(config.enabled, true);
        assert_eq!(config.interval_seconds, 60);
        assert_eq!(config.batch_size, 5);
        assert_eq!(config.skip_static_frames, true);
        assert_eq!(config.max_queue_size, 100);
    }

    #[test]
    fn test_should_use_regions() {
        // Empty regions
        assert!(!OcrProcessor::should_use_regions(&[]));

        // Too many regions
        let many_regions: Vec<BoundingBox> = (0..15)
            .map(|i| BoundingBox::new(i * 10, i * 10, 50, 50))
            .collect();
        assert!(!OcrProcessor::should_use_regions(&many_regions));

        // One very large region (likely video)
        let large_region = vec![BoundingBox::new(0, 0, 1920, 1080)];
        assert!(!OcrProcessor::should_use_regions(&large_region));

        // Reasonable regions
        let good_regions = vec![
            BoundingBox::new(100, 100, 200, 100),
            BoundingBox::new(400, 200, 300, 150),
        ];
        assert!(OcrProcessor::should_use_regions(&good_regions));
    }

    #[test]
    fn test_merge_ocr_results() {
        let result1 = OcrResult::new(0, vec![], 100);
        let mut result2 = OcrResult::new(0, vec![], 150);
        result2.total_text = "Hello".to_string();

        let merged = OcrProcessor::merge_ocr_results(vec![result1, result2]);

        assert_eq!(merged.processing_time_ms, 250);
        assert_eq!(merged.total_text, "Hello");
    }
}
