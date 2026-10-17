//! The Public Suffix List (spec §9.7; owner decision 2026-10-16: the
//! official list is vendored, no library). `public_suffix_list.dat` is the
//! file from publicsuffix.org, committed with its `VERSION` line and
//! updated deliberately. The algorithm is the list's own
//! (publicsuffix.org/list): the longest matching rule wins, an exception
//! rule (`!x`) beats a wildcard (`*.x`), and the default rule is `*`.
//!
//! Hosts here are ASCII (punycode). The list writes internationalized
//! suffixes in Unicode, which an ASCII host never matches; so a host whose
//! suffix part is punycode is treated as having no registrable domain —
//! fail closed (`registrable` → `None`), never a wider match.

use std::collections::HashSet;
use std::sync::OnceLock;

const LIST: &str = include_str!("public_suffix_list.dat");

struct Rules {
    normal: HashSet<&'static str>,
    wildcard: HashSet<&'static str>,
    exception: HashSet<&'static str>,
}

fn rules() -> &'static Rules {
    static R: OnceLock<Rules> = OnceLock::new();
    R.get_or_init(|| {
        let mut r = Rules { normal: HashSet::new(), wildcard: HashSet::new(), exception: HashSet::new() };
        for line in LIST.lines() {
            let rule = line.split_whitespace().next().unwrap_or("");
            if rule.is_empty() || rule.starts_with("//") {
                continue;
            }
            if let Some(x) = rule.strip_prefix('!') {
                r.exception.insert(x);
            } else if let Some(x) = rule.strip_prefix("*.") {
                r.wildcard.insert(x);
            } else {
                r.normal.insert(rule);
            }
        }
        r
    })
}

/// The list's version line, for the record (`VERSION: …`).
pub fn version() -> &'static str {
    LIST.lines().find_map(|l| l.strip_prefix("// VERSION: ")).unwrap_or("unknown")
}

/// How many trailing labels of `host` are its public suffix.
fn suffix_labels(labels: &[&str]) -> usize {
    let r = rules();
    let n = labels.len();
    let mut best = 1; // the default rule "*"
    for i in 0..n {
        let tail = labels[i..].join(".");
        let len = n - i;
        if r.exception.contains(tail.as_str()) {
            return len - 1; // an exception: the suffix is one label shorter
        }
        if r.normal.contains(tail.as_str()) {
            best = best.max(len);
        }
        if i > 0 && r.wildcard.contains(tail.as_str()) {
            best = best.max(len + 1);
        }
    }
    best
}

/// The registrable domain (public suffix + one label), or `None` when the
/// host is itself a public suffix, is not ASCII, or its suffix is punycode.
pub fn registrable(host: &str) -> Option<String> {
    // An IP literal has no registrable domain (final review VER-I4): the
    // list's rules would otherwise make 45.33.1.5 and 192.168.1.5 "1.5".
    if !host.is_ascii() || host.is_empty() || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    let s = suffix_labels(&labels);
    if s >= labels.len() || labels[labels.len() - s..].iter().any(|l| l.starts_with("xn--")) {
        return None;
    }
    Some(labels[labels.len() - s - 1..].join("."))
}

#[cfg(test)]
mod tests {
    use super::registrable;

    #[test]
    fn the_boundaries_the_spec_names() {
        assert_eq!(registrable("github.com").as_deref(), Some("github.com"));
        assert_eq!(registrable("gist.github.com").as_deref(), Some("github.com"));
        assert_eq!(registrable("alice.github.io").as_deref(), Some("alice.github.io"));
        assert_eq!(registrable("github.io"), None, "a public suffix has no registrable domain");
        assert_eq!(registrable("bbc.co.uk").as_deref(), Some("bbc.co.uk"));
        assert_eq!(registrable("co.uk"), None);
        assert_eq!(registrable("app.appspot.com").as_deref(), Some("app.appspot.com"));
        assert!(super::version().starts_with("20"));
        assert_eq!(registrable("192.168.1.5"), None, "an IP literal has none");
        assert_eq!(registrable("45.33.1.5"), None);
    }

    #[test]
    fn wildcards_and_exceptions() {
        // "*.ck" with exception "!www.ck" in the list.
        assert_eq!(registrable("a.b.ck").as_deref(), Some("a.b.ck"));
        assert_eq!(registrable("www.ck").as_deref(), Some("www.ck"));
    }
}
