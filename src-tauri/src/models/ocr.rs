// OCR (Optical Character Recognition) data models

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Represents a bounding box for text regions in an image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl BoundingBox {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Calculate the area of the bounding box
    pub fn area(&self) -> u32 {
        self.width * self.height
    }

    /// Check if a point (x, y) is inside this bounding box
    pub fn contains_point(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Check if this bounding box overlaps with another
    pub fn overlaps_with(&self, other: &BoundingBox) -> bool {
        !(self.x + self.width < other.x
            || other.x + other.width < self.x
            || self.y + self.height < other.y
            || other.y + other.height < self.y)
    }
}

/// Represents a block of text detected in an image with its location and confidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
    pub confidence: f32, // 0.0 to 1.0
    pub bounding_box: BoundingBox,
    pub language: String,
}

impl TextBlock {
    pub fn new(
        text: String,
        confidence: f32,
        bounding_box: BoundingBox,
        language: String,
    ) -> Self {
        Self {
            text,
            confidence,
            bounding_box,
            language,
        }
    }

    /// Check if the confidence meets a threshold
    pub fn meets_confidence(&self, threshold: f32) -> bool {
        self.confidence >= threshold
    }

    /// Get the text trimmed of whitespace
    pub fn trimmed_text(&self) -> String {
        self.text.trim().to_string()
    }
}

/// Result of OCR processing on a single frame
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub timestamp: i64,
    pub frame_path: Option<PathBuf>,
    pub text_blocks: Vec<TextBlock>,
    pub processing_time_ms: u64,
    pub total_text: String, // All text concatenated with spaces
}

impl OcrResult {
    pub fn new(timestamp: i64, text_blocks: Vec<TextBlock>, processing_time_ms: u64) -> Self {
        let total_text = text_blocks
            .iter()
            .map(|b| b.trimmed_text())
            .collect::<Vec<_>>()
            .join(" ");

        Self {
            timestamp,
            frame_path: None,
            text_blocks,
            processing_time_ms,
            total_text,
        }
    }

    /// Get all text with confidence above threshold
    pub fn get_high_confidence_text(&self, threshold: f32) -> String {
        self.text_blocks
            .iter()
            .filter(|b| b.meets_confidence(threshold))
            .map(|b| b.trimmed_text())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Get the number of detected text blocks
    pub fn block_count(&self) -> usize {
        self.text_blocks.len()
    }

    /// Get the average confidence of all text blocks
    pub fn average_confidence(&self) -> f32 {
        if self.text_blocks.is_empty() {
            return 0.0;
        }

        let sum: f32 = self.text_blocks.iter().map(|b| b.confidence).sum();
        sum / self.text_blocks.len() as f32
    }

    /// Check if any text was detected
    pub fn has_text(&self) -> bool {
        !self.text_blocks.is_empty() && !self.total_text.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounding_box_area() {
        let bbox = BoundingBox::new(10, 20, 100, 50);
        assert_eq!(bbox.area(), 5000);
    }

    #[test]
    fn test_bounding_box_contains_point() {
        let bbox = BoundingBox::new(10, 20, 100, 50);
        assert!(bbox.contains_point(50, 40));
        assert!(!bbox.contains_point(5, 5));
        assert!(!bbox.contains_point(150, 100));
    }

    #[test]
    fn test_bounding_box_overlaps() {
        let bbox1 = BoundingBox::new(10, 20, 100, 50);
        let bbox2 = BoundingBox::new(50, 40, 80, 60);
        let bbox3 = BoundingBox::new(200, 200, 50, 50);

        assert!(bbox1.overlaps_with(&bbox2));
        assert!(!bbox1.overlaps_with(&bbox3));
    }

    #[test]
    fn test_text_block_confidence() {
        let text_block = TextBlock::new(
            "Hello".to_string(),
            0.85,
            BoundingBox::new(0, 0, 100, 20),
            "eng".to_string(),
        );

        assert!(text_block.meets_confidence(0.8));
        assert!(!text_block.meets_confidence(0.9));
    }

    #[test]
    fn test_ocr_result_high_confidence_text() {
        let blocks = vec![
            TextBlock::new(
                "Hello".to_string(),
                0.9,
                BoundingBox::new(0, 0, 100, 20),
                "eng".to_string(),
            ),
            TextBlock::new(
                "World".to_string(),
                0.5,
                BoundingBox::new(100, 0, 100, 20),
                "eng".to_string(),
            ),
            TextBlock::new(
                "Test".to_string(),
                0.95,
                BoundingBox::new(200, 0, 100, 20),
                "eng".to_string(),
            ),
        ];

        let result = OcrResult::new(0, blocks, 100);

        assert_eq!(result.get_high_confidence_text(0.8), "Hello Test");
        assert_eq!(result.block_count(), 3);
        assert_eq!(result.average_confidence(), 0.783333333);
    }

    #[test]
    fn test_ocr_result_has_text() {
        let blocks_with_text = vec![TextBlock::new(
            "Hello".to_string(),
            0.9,
            BoundingBox::new(0, 0, 100, 20),
            "eng".to_string(),
        )];

        let result_with_text = OcrResult::new(0, blocks_with_text, 100);
        assert!(result_with_text.has_text());

        let empty_result = OcrResult::new(0, vec![], 100);
        assert!(!empty_result.has_text());
    }
}
