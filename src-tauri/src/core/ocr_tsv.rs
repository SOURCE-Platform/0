// Turns Tesseract's TSV output into positioned text lines.
//
// `get_text()` only returns one flat string, which left every OCR capture as a
// single block with a made-up 0,0,100,100 box and a hard-coded confidence, so
// the timeline's reconstructed scene piled all text into one corner. The TSV
// output carries a pixel box and confidence for every word; words are grouped
// back into lines here so each on-screen line becomes its own block.

use crate::models::ocr::{BoundingBox, TextBlock};
use std::collections::BTreeMap;

/// Tesseract TSV row level for a single word.
const WORD_LEVEL: &str = "5";

struct Line {
    words: Vec<String>,
    confidence_sum: f32,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

/// Group word rows by (page, block, paragraph, line) into text blocks, in
/// reading order. Words Tesseract could not score (conf -1) or that are blank
/// are skipped. Confidence is the mean word confidence, scaled to 0..1.
pub fn lines_from_tsv(tsv: &str, language: &str) -> Vec<TextBlock> {
    let mut lines: BTreeMap<(u32, u32, u32, u32), Line> = BTreeMap::new();

    for row in tsv.lines() {
        let cols: Vec<&str> = row.split('\t').collect();
        if cols.len() < 12 || cols[0] != WORD_LEVEL {
            continue;
        }
        let text = cols[11..].join("\t");
        let text = text.trim();
        let numbers: Option<Vec<u32>> = cols[1..10].iter().map(|v| v.parse().ok()).collect();
        let (Some(numbers), Ok(confidence)) = (numbers, cols[10].parse::<f32>()) else {
            continue;
        };
        if text.is_empty() || confidence < 0.0 {
            continue;
        }
        let [page, block, par, line, _word, left, top, width, height] = numbers[..] else {
            continue;
        };

        let entry = lines.entry((page, block, par, line)).or_insert(Line {
            words: Vec::new(),
            confidence_sum: 0.0,
            left,
            top,
            right: left + width,
            bottom: top + height,
        });
        entry.words.push(text.to_string());
        entry.confidence_sum += confidence;
        entry.left = entry.left.min(left);
        entry.top = entry.top.min(top);
        entry.right = entry.right.max(left + width);
        entry.bottom = entry.bottom.max(top + height);
    }

    lines
        .into_values()
        .map(|line| {
            let confidence = line.confidence_sum / line.words.len() as f32 / 100.0;
            TextBlock::new(
                line.words.join(" "),
                confidence,
                BoundingBox::new(line.left, line.top, line.right - line.left, line.bottom - line.top),
                language.to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

    #[test]
    fn groups_words_into_positioned_lines() {
        let tsv = format!(
            "{HEADER}\n\
             4\t1\t1\t1\t1\t0\t10\t20\t300\t30\t-1\t\n\
             5\t1\t1\t1\t1\t1\t10\t20\t80\t30\t96\tTimeline\n\
             5\t1\t1\t1\t1\t2\t100\t22\t40\t28\t90\tID\n\
             5\t1\t1\t1\t2\t1\t12\t70\t120\t30\t80\tSettings\n"
        );
        let blocks = lines_from_tsv(&tsv, "eng");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "Timeline ID");
        assert_eq!(blocks[0].bounding_box.x, 10);
        assert_eq!(blocks[0].bounding_box.y, 20);
        assert_eq!(blocks[0].bounding_box.width, 130);
        assert_eq!(blocks[0].bounding_box.height, 30);
        assert!((blocks[0].confidence - 0.93).abs() < 0.001);
        assert_eq!(blocks[1].text, "Settings");
        assert_eq!(blocks[1].bounding_box.y, 70);
    }

    #[test]
    fn skips_unscored_blank_and_malformed_rows() {
        let tsv = format!(
            "{HEADER}\n\
             5\t1\t1\t1\t1\t1\t0\t0\t10\t10\t-1\tghost\n\
             5\t1\t1\t1\t1\t2\t0\t0\t10\t10\t90\t   \n\
             5\tnot\ta\tnumber\t1\t1\t0\t0\t10\t10\t90\tbad\n"
        );
        assert!(lines_from_tsv(&tsv, "eng").is_empty());
    }
}
