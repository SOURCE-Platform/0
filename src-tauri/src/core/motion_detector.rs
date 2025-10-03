// Motion detection - identifies when screen content changes

use crate::models::capture::RawFrame;

/// Bounding box representing a region with motion
#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Result of motion detection
#[derive(Debug, Clone)]
pub struct MotionResult {
    pub has_motion: bool,
    pub changed_percentage: f32,
    pub bounding_boxes: Vec<BoundingBox>,
}

/// Motion detector that compares frames to detect changes
pub struct MotionDetector {
    previous_frame: Option<Vec<u8>>,
    previous_dimensions: Option<(u32, u32)>,
    threshold: f32,
    pixel_diff_threshold: u8,
}

impl MotionDetector {
    /// Create a new motion detector
    ///
    /// # Arguments
    /// * `threshold` - Percentage of changed pixels to trigger motion (0.0-1.0)
    ///   e.g., 0.05 = 5% of pixels must change
    pub fn new(threshold: f32) -> Self {
        Self {
            previous_frame: None,
            previous_dimensions: None,
            threshold,
            pixel_diff_threshold: 10, // RGB diff threshold per pixel
        }
    }

    /// Detect motion by comparing current frame with previous
    pub fn detect_motion(&mut self, current_frame: &RawFrame) -> MotionResult {
        let current_dims = (current_frame.width, current_frame.height);

        // First frame or dimension change - always has "motion"
        if self.previous_frame.is_none() || self.previous_dimensions != Some(current_dims) {
            self.previous_frame = Some(current_frame.data.clone());
            self.previous_dimensions = Some(current_dims);

            return MotionResult {
                has_motion: true,
                changed_percentage: 1.0,
                bounding_boxes: vec![BoundingBox {
                    x: 0,
                    y: 0,
                    width: current_frame.width,
                    height: current_frame.height,
                }],
            };
        }

        let previous_data = self.previous_frame.as_ref().unwrap();
        let changed_pixels = self.count_changed_pixels(previous_data, &current_frame.data);

        let total_pixels = (current_frame.width * current_frame.height) as usize;
        let changed_percentage = changed_pixels as f32 / total_pixels as f32;

        let has_motion = changed_percentage >= self.threshold;

        // Calculate bounding boxes if there's motion
        let bounding_boxes = if has_motion {
            self.calculate_bounding_boxes(
                previous_data,
                &current_frame.data,
                current_frame.width,
                current_frame.height,
            )
        } else {
            Vec::new()
        };

        // Update previous frame
        self.previous_frame = Some(current_frame.data.clone());

        MotionResult {
            has_motion,
            changed_percentage,
            bounding_boxes,
        }
    }

    /// Reset the detector (clears previous frame)
    pub fn reset(&mut self) {
        self.previous_frame = None;
        self.previous_dimensions = None;
    }

    /// Count pixels that have changed between two frames
    fn count_changed_pixels(&self, previous: &[u8], current: &[u8]) -> usize {
        let mut changed = 0;

        for i in (0..previous.len()).step_by(4) {
            if i + 3 < current.len() {
                let prev_r = previous[i];
                let prev_g = previous[i + 1];
                let prev_b = previous[i + 2];

                let curr_r = current[i];
                let curr_g = current[i + 1];
                let curr_b = current[i + 2];

                // Calculate RGB difference
                let diff_r = (prev_r as i16 - curr_r as i16).abs() as u8;
                let diff_g = (prev_g as i16 - curr_g as i16).abs() as u8;
                let diff_b = (prev_b as i16 - curr_b as i16).abs() as u8;

                // If any channel differs significantly, count as changed
                if diff_r > self.pixel_diff_threshold
                    || diff_g > self.pixel_diff_threshold
                    || diff_b > self.pixel_diff_threshold
                {
                    changed += 1;
                }
            }
        }

        changed
    }

    /// Calculate bounding boxes for regions with motion
    /// Divides screen into grid and finds regions with changes
    fn calculate_bounding_boxes(
        &self,
        previous: &[u8],
        current: &[u8],
        width: u32,
        height: u32,
    ) -> Vec<BoundingBox> {
        // Divide screen into 10x10 grid
        let grid_size = 10;
        let cell_width = width / grid_size;
        let cell_height = height / grid_size;

        let mut changed_cells = Vec::new();

        // Check each grid cell for changes
        for grid_y in 0..grid_size {
            for grid_x in 0..grid_size {
                let x_start = grid_x * cell_width;
                let y_start = grid_y * cell_height;
                let x_end = ((grid_x + 1) * cell_width).min(width);
                let y_end = ((grid_y + 1) * cell_height).min(height);

                if self.cell_has_motion(previous, current, width, x_start, y_start, x_end, y_end) {
                    changed_cells.push((grid_x, grid_y));
                }
            }
        }

        // Merge adjacent cells into bounding boxes
        self.merge_cells_to_boxes(&changed_cells, cell_width, cell_height)
    }

    /// Check if a cell has motion
    fn cell_has_motion(
        &self,
        previous: &[u8],
        current: &[u8],
        width: u32,
        x_start: u32,
        y_start: u32,
        x_end: u32,
        y_end: u32,
    ) -> bool {
        let mut changed_in_cell = 0;
        let mut total_in_cell = 0;

        for y in y_start..y_end {
            for x in x_start..x_end {
                let pixel_index = ((y * width + x) * 4) as usize;

                if pixel_index + 3 < previous.len() && pixel_index + 3 < current.len() {
                    total_in_cell += 1;

                    let prev_r = previous[pixel_index];
                    let prev_g = previous[pixel_index + 1];
                    let prev_b = previous[pixel_index + 2];

                    let curr_r = current[pixel_index];
                    let curr_g = current[pixel_index + 1];
                    let curr_b = current[pixel_index + 2];

                    let diff_r = (prev_r as i16 - curr_r as i16).abs() as u8;
                    let diff_g = (prev_g as i16 - curr_g as i16).abs() as u8;
                    let diff_b = (prev_b as i16 - curr_b as i16).abs() as u8;

                    if diff_r > self.pixel_diff_threshold
                        || diff_g > self.pixel_diff_threshold
                        || diff_b > self.pixel_diff_threshold
                    {
                        changed_in_cell += 1;
                    }
                }
            }
        }

        // Cell has motion if more than 5% of its pixels changed
        total_in_cell > 0 && (changed_in_cell as f32 / total_in_cell as f32) > 0.05
    }

    /// Merge adjacent cells into bounding boxes
    fn merge_cells_to_boxes(
        &self,
        cells: &[(u32, u32)],
        cell_width: u32,
        cell_height: u32,
    ) -> Vec<BoundingBox> {
        if cells.is_empty() {
            return Vec::new();
        }

        // Simple approach: create one bounding box per contiguous region
        // For now, just create individual boxes for each cell
        // TODO: Implement proper region merging algorithm

        cells
            .iter()
            .map(|(grid_x, grid_y)| BoundingBox {
                x: grid_x * cell_width,
                y: grid_y * cell_height,
                width: cell_width,
                height: cell_height,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::capture::PixelFormat;

    fn create_test_frame(width: u32, height: u32, color: [u8; 4]) -> RawFrame {
        let pixel_count = (width * height) as usize;
        let mut data = Vec::with_capacity(pixel_count * 4);

        for _ in 0..pixel_count {
            data.extend_from_slice(&color);
        }

        RawFrame {
            timestamp: 0,
            width,
            height,
            data,
            format: PixelFormat::RGBA8,
        }
    }

    #[test]
    fn test_first_frame_has_motion() {
        let mut detector = MotionDetector::new(0.05);
        let frame = create_test_frame(100, 100, [255, 0, 0, 255]); // Red

        let result = detector.detect_motion(&frame);

        assert!(result.has_motion, "First frame should always have motion");
        assert_eq!(result.changed_percentage, 1.0);
    }

    #[test]
    fn test_identical_frames_no_motion() {
        let mut detector = MotionDetector::new(0.05);

        let frame1 = create_test_frame(100, 100, [255, 0, 0, 255]);
        let frame2 = create_test_frame(100, 100, [255, 0, 0, 255]);

        detector.detect_motion(&frame1);
        let result = detector.detect_motion(&frame2);

        assert!(!result.has_motion, "Identical frames should have no motion");
        assert_eq!(result.changed_percentage, 0.0);
    }

    #[test]
    fn test_completely_different_frames() {
        let mut detector = MotionDetector::new(0.05);

        let frame1 = create_test_frame(100, 100, [255, 0, 0, 255]); // Red
        let frame2 = create_test_frame(100, 100, [0, 0, 255, 255]); // Blue

        detector.detect_motion(&frame1);
        let result = detector.detect_motion(&frame2);

        assert!(result.has_motion, "Completely different frames should have motion");
        assert!(result.changed_percentage > 0.99);
    }

    #[test]
    fn test_threshold_sensitivity() {
        let mut detector_sensitive = MotionDetector::new(0.01); // 1% threshold
        let mut detector_insensitive = MotionDetector::new(0.50); // 50% threshold

        // Create frame with small change (5% of pixels)
        let frame1 = create_test_frame(100, 100, [255, 0, 0, 255]);
        let mut frame2 = frame1.clone();

        // Change 5% of pixels
        for i in 0..500 {
            let idx = i * 4;
            if idx + 3 < frame2.data.len() {
                frame2.data[idx] = 0; // Change red to black
                frame2.data[idx + 1] = 0;
                frame2.data[idx + 2] = 0;
            }
        }

        detector_sensitive.detect_motion(&frame1);
        let result_sensitive = detector_sensitive.detect_motion(&frame2);

        detector_insensitive.detect_motion(&frame1);
        let result_insensitive = detector_insensitive.detect_motion(&frame2);

        assert!(
            result_sensitive.has_motion,
            "Sensitive detector should detect small motion"
        );
        assert!(
            !result_insensitive.has_motion,
            "Insensitive detector should not detect small motion"
        );
    }
}
