// OCR (Optical Character Recognition) engine using Tesseract

use crate::models::capture::RawFrame;
use crate::models::ocr::{BoundingBox, OcrResult, TextBlock};
use image::{GrayImage, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tesseract::Tesseract;
use thiserror::Error;
use uuid::Uuid;

// ==============================================================================
// Errors
// ==============================================================================

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("Tesseract initialization failed: {0}")]
    TesseractInit(String),

    #[error("Image conversion failed")]
    ImageConversion,

    #[error("OCR processing failed: {0}")]
    Processing(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
}

type Result<T> = std::result::Result<T, OcrError>;

// ==============================================================================
// Configuration
// ==============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrConfig {
    pub languages: Vec<String>,       // ["eng", "spa", "fra"]
    pub psm: u32,                     // Page segmentation mode (0-13)
    pub oem: u32,                     // OCR Engine mode (0-3)
    pub dpi: u32,                     // DPI for processing
    pub confidence_threshold: f32,    // Minimum confidence (0.0-1.0)
    pub preprocess_enabled: bool,     // Enable image preprocessing
    pub contrast_factor: f32,         // Contrast adjustment factor (1.0 = no change)
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            languages: vec!["eng".to_string()],
            psm: 3,  // Fully automatic page segmentation
            oem: 3,  // Default, based on what is available
            dpi: 300,
            confidence_threshold: 0.6,
            preprocess_enabled: true,
            contrast_factor: 1.5,
        }
    }
}

impl OcrConfig {
    /// Create config with custom languages
    pub fn with_languages(languages: Vec<String>) -> Self {
        Self {
            languages,
            ..Default::default()
        }
    }

    /// Create config optimized for screenshots
    pub fn for_screenshots() -> Self {
        Self {
            languages: vec!["eng".to_string()],
            psm: 3,  // Fully automatic
            oem: 3,  // Default
            dpi: 144, // Typical screen DPI
            confidence_threshold: 0.7,
            preprocess_enabled: true,
            contrast_factor: 1.3,
        }
    }

    /// Create config optimized for documents
    pub fn for_documents() -> Self {
        Self {
            languages: vec!["eng".to_string()],
            psm: 1,  // Automatic page segmentation with OSD (orientation and script detection)
            oem: 3,
            dpi: 300,
            confidence_threshold: 0.8,
            preprocess_enabled: true,
            contrast_factor: 1.5,
        }
    }
}

// ==============================================================================
// OCR Engine
// ==============================================================================

pub struct OcrEngine {
    config: OcrConfig,
}

impl OcrEngine {
    /// Create a new OCR engine with the given configuration
    pub fn new(config: OcrConfig) -> Result<Self> {
        // Validate that Tesseract is available by attempting to create an instance
        Self::validate_tesseract(&config)?;

        Ok(Self { config })
    }

    /// Create a new OCR engine with default configuration
    pub fn with_default() -> Result<Self> {
        Self::new(OcrConfig::default())
    }

    /// Validate that Tesseract is available and can be initialized
    fn validate_tesseract(config: &OcrConfig) -> Result<()> {
        let languages = config.languages.join("+");

        Tesseract::new(None, Some(&languages))
            .map_err(|e| OcrError::TesseractInit(e.to_string()))?;

        Ok(())
    }

    /// Extract text from a captured frame
    pub async fn extract_text_from_frame(&self, frame: &RawFrame) -> Result<OcrResult> {
        let start_time = std::time::Instant::now();

        // Convert frame to image
        let image = self.frame_to_image(frame)?;

        // Preprocess if enabled
        let processed = if self.config.preprocess_enabled {
            self.preprocess_image(&image)?
        } else {
            image::imageops::grayscale(&image)
        };

        // Save to temporary file (Tesseract works best with files)
        let temp_path = self.save_temp_image(&processed)?;

        // Run OCR
        let text_blocks = self.run_ocr(&temp_path)?;

        // Filter by confidence
        let filtered_blocks: Vec<TextBlock> = text_blocks
            .into_iter()
            .filter(|b| b.confidence >= self.config.confidence_threshold)
            .collect();

        let processing_time = start_time.elapsed();

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);

        let mut result = OcrResult::new(
            chrono::Utc::now().timestamp_millis(),
            filtered_blocks,
            processing_time.as_millis() as u64,
        );
        result.frame_path = Some(temp_path);

        Ok(result)
    }

    /// Extract text from a specific region of a frame
    pub async fn extract_text_from_region(
        &self,
        frame: &RawFrame,
        region: &BoundingBox,
    ) -> Result<OcrResult> {
        // Crop frame to region
        let cropped = self.crop_frame(frame, region)?;

        // Run OCR on cropped region
        self.extract_text_from_frame(&cropped).await
    }

    /// Convert a RawFrame to an RgbaImage
    fn frame_to_image(&self, frame: &RawFrame) -> Result<RgbaImage> {
        RgbaImage::from_raw(frame.width, frame.height, frame.data.clone())
            .ok_or(OcrError::ImageConversion)
    }

    /// Preprocess image for better OCR accuracy
    fn preprocess_image(&self, img: &RgbaImage) -> Result<GrayImage> {
        // Convert to grayscale
        let gray = image::imageops::grayscale(img);

        // Increase contrast
        let contrasted = self.adjust_contrast(&gray, self.config.contrast_factor);

        Ok(contrasted)
    }

    /// Adjust image contrast
    fn adjust_contrast(&self, img: &GrayImage, factor: f32) -> GrayImage {
        let mut output = img.clone();

        for pixel in output.pixels_mut() {
            let value = pixel[0] as f32;
            let adjusted = ((value - 128.0) * factor + 128.0).clamp(0.0, 255.0);
            pixel[0] = adjusted as u8;
        }

        output
    }

    /// Save image to temporary file
    fn save_temp_image(&self, img: &GrayImage) -> Result<PathBuf> {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("ocr_{}.png", Uuid::new_v4()));

        img.save(&temp_path)?;

        Ok(temp_path)
    }

    /// Run Tesseract OCR on an image file
    fn run_ocr(&self, image_path: &PathBuf) -> Result<Vec<TextBlock>> {
        let languages = self.config.languages.join("+");

        let mut tesseract = Tesseract::new(None, Some(&languages))
            .map_err(|e| OcrError::TesseractInit(e.to_string()))?
            .set_variable("tessedit_pageseg_mode", &self.config.psm.to_string())
            .map_err(|e| OcrError::Processing(e.to_string()))?
            .set_variable("tessedit_ocr_engine_mode", &self.config.oem.to_string())
            .map_err(|e| OcrError::Processing(e.to_string()))?
            .set_variable("user_defined_dpi", &self.config.dpi.to_string())
            .map_err(|e| OcrError::Processing(e.to_string()))?
            .set_image(image_path.to_str().unwrap())
            .map_err(|e| OcrError::Processing(e.to_string()))?;

        // Get text
        let text = tesseract
            .get_text()
            .map_err(|e| OcrError::Processing(e.to_string()))?;

        // For now, create a single text block with the full text
        // The tesseract crate v0.14 doesn't expose detailed bounding box API
        // We'll return the entire text as one block
        let text_blocks = if !text.trim().is_empty() {
            vec![TextBlock::new(
                text.trim().to_string(),
                0.85, // Default confidence since we don't have per-word data
                BoundingBox::new(0, 0, 100, 100), // Placeholder bounding box
                self.config.languages[0].clone(),
            )]
        } else {
            vec![]
        };

        Ok(text_blocks)
    }

    /// Crop a frame to a specific region
    fn crop_frame(&self, frame: &RawFrame, region: &BoundingBox) -> Result<RawFrame> {
        let img = self.frame_to_image(frame)?;

        let cropped = image::imageops::crop_imm(&img, region.x, region.y, region.width, region.height)
            .to_image();

        Ok(RawFrame {
            timestamp: frame.timestamp,
            width: region.width,
            height: region.height,
            data: cropped.into_raw(),
            format: frame.format.clone(),
        })
    }

    /// Get list of supported languages (commonly available)
    pub fn supported_languages() -> Vec<String> {
        vec![
            "eng".to_string(),      // English
            "spa".to_string(),      // Spanish
            "fra".to_string(),      // French
            "deu".to_string(),      // German
            "ita".to_string(),      // Italian
            "por".to_string(),      // Portuguese
            "rus".to_string(),      // Russian
            "chi_sim".to_string(),  // Chinese Simplified
            "chi_tra".to_string(),  // Chinese Traditional
            "jpn".to_string(),      // Japanese
            "kor".to_string(),      // Korean
            "ara".to_string(),      // Arabic
            "hin".to_string(),      // Hindi
        ]
    }

    /// Update the languages used by the OCR engine
    pub fn set_languages(&mut self, languages: Vec<String>) -> Result<()> {
        // Validate the new configuration
        let new_config = OcrConfig {
            languages: languages.clone(),
            ..self.config.clone()
        };

        Self::validate_tesseract(&new_config)?;

        self.config.languages = languages;
        Ok(())
    }

    /// Get the current configuration
    pub fn config(&self) -> &OcrConfig {
        &self.config
    }

    /// Update the configuration
    pub fn set_config(&mut self, config: OcrConfig) -> Result<()> {
        Self::validate_tesseract(&config)?;
        self.config = config;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_config_default() {
        let config = OcrConfig::default();
        assert_eq!(config.languages, vec!["eng"]);
        assert_eq!(config.psm, 3);
        assert_eq!(config.confidence_threshold, 0.6);
    }

    #[test]
    fn test_ocr_config_screenshots() {
        let config = OcrConfig::for_screenshots();
        assert_eq!(config.dpi, 144);
        assert_eq!(config.confidence_threshold, 0.7);
    }

    #[test]
    fn test_supported_languages() {
        let languages = OcrEngine::supported_languages();
        assert!(languages.contains(&"eng".to_string()));
        assert!(languages.contains(&"spa".to_string()));
        assert!(languages.contains(&"fra".to_string()));
    }

    #[test]
    fn test_contrast_adjustment() {
        let engine = OcrEngine::with_default().unwrap();

        // Create a simple grayscale image
        let img = GrayImage::new(100, 100);

        // Adjust contrast
        let adjusted = engine.adjust_contrast(&img, 1.5);

        // Verify image dimensions are preserved
        assert_eq!(adjusted.width(), 100);
        assert_eq!(adjusted.height(), 100);
    }
}
