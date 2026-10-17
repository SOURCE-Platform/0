//! Confusable skeletons (spec §9.5; BR-05/BR-06). The helper accepts
//! ASCII host names only (`origin.rs`), so the skeleton needs only the
//! ASCII-to-ASCII entries of Unicode `confusables.txt` (UTS #39):
//! `rn`→`m`, `vv`→`w`, `cl`→`d`, `0`→`o`, `1`→`l`, `|`→`l`. A non-ASCII
//! look-alike reaches the helper only as punycode, which §9.5 refuses on
//! its own unless the record was saved for exactly that host.
//!
//! A page whose registrable domain has the same skeleton as a stored,
//! *different* one is a look-alike: it is refused (it would be anyway —
//! different host) and the extension shows the warning banner.

use super::origin::Origin;
use super::psl::registrable;
use crate::storage::records::UrlEntryWire;

pub fn skeleton(host: &str) -> String {
    let lower = host.to_ascii_lowercase();
    let folded = lower.replace("rn", "m").replace("vv", "w").replace("cl", "d");
    folded.chars().map(|c| match c {
        '0' => 'o',
        '1' | '|' => 'l',
        other => other,
    }).collect()
}

fn domain(host: &str) -> String {
    registrable(host).unwrap_or_else(|| host.to_string())
}

/// Does `page` look like one of these stored hosts without being it?
pub fn lookalike(page: &Origin, entries: &[UrlEntryWire]) -> bool {
    let p = domain(&page.host);
    entries.iter().any(|e| {
        let s = domain(e.host.split("://").last().unwrap_or(&e.host));
        s != p && skeleton(&s) == skeleton(&p)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::origin::canonical;

    fn e(host: &str) -> UrlEntryWire {
        UrlEntryWire { host: host.into(), match_mode: "exact".into(), allow_http: false }
    }

    #[test]
    fn ascii_lookalikes() {
        let page = |u: &str| canonical(u).unwrap();
        assert!(lookalike(&page("https://g00gle.example"), &[e("google.example")]), "BR-06");
        assert!(lookalike(&page("https://login.rnicrosoft.test"), &[e("microsoft.test")]));
        assert!(lookalike(&page("https://paypa1.test"), &[e("paypal.test")]));
        assert!(!lookalike(&page("https://google.example"), &[e("google.example")]), "itself is not a look-alike");
        assert!(!lookalike(&page("https://accounts.google.example"), &[e("google.example")]), "same registrable domain");
        assert!(!lookalike(&page("https://github.com"), &[e("google.example")]));
    }
}
