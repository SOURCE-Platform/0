const AUDIO_EMOTION_RAILS: [(&str, &str); 7] = [
    ("happy", "Happy"), ("sad", "Sad"), ("angry", "Angry"),
    ("fearful", "Fearful"), ("surprised", "Surprised"),
    ("neutral", "Neutral"), ("uncertain", "Uncertain"),
];

fn emotion_polarity(label: &str) -> i8 {
    match label {
        "happy" | "surprised" => 1,
        "sad" | "angry" | "fearful" => -1,
        _ => 0,
    }
}

fn polarity_label(polarity: i8) -> &'static str {
    match polarity {
        1 => "Positive",
        -1 => "Negative",
        _ => "Centered",
    }
}

fn prettify_emotion_label(label: &str) -> String {
    label
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
