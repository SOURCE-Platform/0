/// Word Error Rate for the v2-vs-v3 benchmark (Phase 9).
/// Pure function over transcript pairs; recording + model runs come later.
pub fn word_error_rate(reference: &str, hypothesis: &str) -> f32 {
    let reference = normalize_words(reference);
    let hypothesis = normalize_words(hypothesis);
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }
    let distance = levenshtein(&reference, &hypothesis);
    distance as f32 / reference.len() as f32
}

fn normalize_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|char| if char.is_alphanumeric() { char } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn levenshtein(a: &[String], b: &[String]) -> usize {
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, word_a) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, word_b) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(word_a != word_b);
            current.push(substitution.min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_scores_zero() {
        assert_eq!(word_error_rate("hello world", "hello world"), 0.0);
    }

    #[test]
    fn one_substitution_in_four_words() {
        assert!((word_error_rate("a b c d", "a x c d") - 0.25).abs() < 1e-6);
    }

    #[test]
    fn case_and_punctuation_ignored() {
        assert_eq!(word_error_rate("Hello, Kubrick!", "hello kubrick"), 0.0);
    }

    #[test]
    fn empty_reference_with_output_is_full_error() {
        assert_eq!(word_error_rate("", "hello"), 1.0);
        assert_eq!(word_error_rate("", ""), 0.0);
    }
}
