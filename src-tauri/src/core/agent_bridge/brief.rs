//! A reply short enough to read at a glance or hear spoken: plain words, at most
//! two sentences. No model involved, so it's instant and free.

const MAX_CHARS: usize = 280;

/// Brief version of a finished turn. Uses the reply itself; falls back to
/// Claude's own recap when the reply was only code, a table or empty.
pub fn brief_reply(text: &str, summary: Option<&str>) -> String {
    let plain = plain_text(text);
    let brief = first_sentences(&plain, 2);
    if !brief.is_empty() {
        return cap(&brief);
    }
    match summary.map(str::trim).filter(|s| !s.is_empty()) {
        Some(summary) => cap(&capitalize(summary)),
        None => "Done.".to_string(),
    }
}

/// Strip markdown down to the words someone would say out loud.
pub(crate) fn plain_text(markdown: &str) -> String {
    let mut out = Vec::new();
    let mut in_code_block = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        // Code, tables, rules and headings are for reading, not for saying.
        if in_code_block
            || trimmed.starts_with('|')
            || trimmed.starts_with('#')
            || trimmed.chars().all(|c| "-=*_ ".contains(c))
        {
            continue;
        }
        let without_marker = trimmed
            .trim_start_matches("- ")
            .trim_start_matches("* ")
            .trim_start_matches("> ");
        let without_number = strip_list_number(without_marker);
        out.push(strip_inline(without_number));
    }
    out.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_list_number(line: &str) -> &str {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        &line[digits + 2..]
    } else {
        line
    }
}

/// Links keep their words, code keeps its text, emphasis markers go.
fn strip_inline(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match (after.find("]("), after.find(']')) {
            (Some(close), Some(first_close)) if close == first_close => {
                let tail = &after[close + 2..];
                match tail.find(')') {
                    Some(end) => {
                        result.push_str(&rest[..open]);
                        result.push_str(&after[..close]);
                        rest = &tail[end + 1..];
                    }
                    None => break,
                }
            }
            _ => {
                result.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    result.push_str(rest);
    result.replace("**", "").replace("__", "").replace('`', "")
}

/// The first `count` sentences. A sentence ends at . ! or ? followed by a space
/// or the end, so "v2.1.266" and "e.g" in the middle don't split.
pub(crate) fn first_sentences(text: &str, count: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut found = 0;
    for (index, ch) in chars.iter().enumerate() {
        if matches!(ch, '.' | '!' | '?') && chars.get(index + 1).map_or(true, |next| next.is_whitespace()) {
            found += 1;
            if found == count {
                return chars[..=index].iter().collect::<String>().trim().to_string();
            }
        }
    }
    text.trim().to_string()
}

fn cap(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_CHARS - 1).collect();
    let at_word = cut.rfind(' ').map_or(cut.as_str(), |space| &cut[..space]);
    format!("{}…", at_word.trim_end_matches(|c: char| c == ',' || c == ';' || c == ':'))
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => {
            let rest: String = chars.collect();
            let sentence = format!("{}{}", first.to_uppercase(), rest);
            if sentence.ends_with(['.', '!', '?']) { sentence } else { format!("{sentence}.") }
        }
        None => String::new(),
    }
}
