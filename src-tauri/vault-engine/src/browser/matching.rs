//! The matching policy (spec §9.5; Phase H), helper-side and fail closed:
//! a saved URL entry is offered for a page only when this says so.
//!
//! - exact `scheme://host[:port]` → eligible;
//! - a subdomain of the stored host → eligible only with `match:"domain"`,
//!   otherwise a "similar item exists" hint with no autofill;
//! - stored `https`, page `http` → refused, except a local host
//!   (`localhost`, `127.0.0.1`, RFC 1918) on a record with `allow_http`;
//! - a punycode page host → only the exact host it was saved for;
//! - everything else (other hosts, opaque origins, IP tricks) → refused.
//!
//! **Domain mode (§9.5, §9.7):** `match:"domain"` covers every host with
//! the same registrable domain as the stored host, as the vendored Public
//! Suffix List decides (`psl.rs`): `github.com` covers `gist.github.com`;
//! `alice.github.io` never covers `bob.github.io`; a stored host that is
//! itself a public suffix (or whose suffix is punycode) covers nothing
//! beyond itself. In exact mode, a page on the same registrable domain gets
//! the "similar item" hint.

use super::origin::{canonical, Origin};
use super::psl::registrable;
use crate::storage::records::UrlEntryWire;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Eligible,
    /// "A similar item exists" — shown, never filled.
    Similar,
    Refused,
}

/// The stored entry's origin: a bare host means `https`.
fn stored(entry: &UrlEntryWire) -> Option<Origin> {
    let text = if entry.host.contains("://") { entry.host.clone() } else { format!("https://{}", entry.host) };
    canonical(&text).ok()
}

pub fn decide(page: &Origin, entry: &UrlEntryWire) -> Verdict {
    let Some(saved) = stored(entry) else { return Verdict::Refused };
    // Scheme: https pages only, unless the local-http exception applies.
    if page.scheme == "http" && saved.scheme == "https" && !(entry.allow_http && page.is_local()) {
        return Verdict::Refused;
    }
    if page.scheme == "https" && saved.scheme == "http" {
        return Verdict::Refused; // an http-only entry never fills a different scheme
    }
    if page.port != saved.port {
        return Verdict::Refused;
    }
    if page.host == saved.host {
        return Verdict::Eligible;
    }
    // A look-alike (punycode) host is offered only for exactly itself.
    if page.is_punycode() {
        return Verdict::Refused;
    }
    match (registrable(&page.host), registrable(&saved.host)) {
        (Some(p), Some(s)) if p == s => {
            if entry.match_mode == "domain" { Verdict::Eligible } else { Verdict::Similar }
        }
        _ => Verdict::Refused,
    }
}

/// The best verdict over a record's entries.
pub fn best(page: &Origin, entries: &[UrlEntryWire]) -> Verdict {
    let all: Vec<Verdict> = entries.iter().map(|e| decide(page, e)).collect();
    if all.contains(&Verdict::Eligible) {
        Verdict::Eligible
    } else if all.contains(&Verdict::Similar) {
        Verdict::Similar
    } else {
        Verdict::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(host: &str, mode: &str, http: bool) -> UrlEntryWire {
        UrlEntryWire { host: host.into(), match_mode: mode.into(), allow_http: http }
    }

    fn v(page: &str, entry: UrlEntryWire) -> Verdict {
        decide(&canonical(page).unwrap(), &entry)
    }

    #[test]
    fn the_policy_table() {
        assert_eq!(v("https://github.com", e("github.com", "exact", false)), Verdict::Eligible);
        assert_eq!(v("https://gist.github.com", e("github.com", "exact", false)), Verdict::Similar);
        assert_eq!(v("https://gist.github.com", e("github.com", "domain", false)), Verdict::Eligible, "same registrable domain");
        assert_eq!(v("https://github.com", e("login.github.com", "domain", false)), Verdict::Eligible, "a sibling, same registrable domain");
        assert_eq!(v("https://github.com", e("login.github.com", "exact", false)), Verdict::Similar);
        assert_eq!(v("https://bob.github.io", e("github.io", "domain", false)), Verdict::Refused, "a public suffix covers nobody");
        assert_eq!(v("https://a.login.example.com", e("login.example.com", "domain", false)), Verdict::Eligible);
        assert_eq!(v("https://evilgithub.com", e("github.com", "domain", false)), Verdict::Refused);
        assert_eq!(v("https://github.com.evil.test", e("github.com", "domain", false)), Verdict::Refused);
        assert_eq!(v("http://github.com", e("github.com", "exact", false)), Verdict::Refused);
        assert_eq!(v("http://192.168.1.10", e("192.168.1.10", "exact", true)), Verdict::Eligible);
        assert_eq!(v("http://192.168.1.10", e("192.168.1.10", "exact", false)), Verdict::Refused);
        assert_eq!(v("https://github.com:8443", e("github.com", "exact", false)), Verdict::Refused);
        assert_eq!(v("https://xn--gthub-cta.com", e("github.com", "domain", false)), Verdict::Refused);
        assert_eq!(v("https://xn--gthub-cta.com", e("xn--gthub-cta.com", "exact", false)), Verdict::Eligible);
        assert_eq!(v("https://alice.github.io", e("bob.github.io", "domain", false)), Verdict::Refused);
    }
}
