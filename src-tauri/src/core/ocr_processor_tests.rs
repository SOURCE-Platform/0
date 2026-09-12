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
