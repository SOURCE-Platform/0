//! Header detection and field mapping (spec §10.2; Phase I). The header
//! row decides the dialect (case-insensitive, trimmed). A row maps to the
//! field bag `add_item` takes, or is skipped with a reason that names a
//! category, never a value.
//!
//! - credentials → `login@1`; `otpSecret` present → counted "re-enter
//!   manually", never imported;
//! - payments → `card@1`; `cvv`/`cvc`/`security_code` are discarded on
//!   sight (the column is never read);
//! - secure notes → `secure_note@1` (§10.8 Choice A).

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Login,
    Card,
    Note,
}

pub struct Dialect {
    pub kind: Kind,
    cols: Vec<String>,
}

/// What became of one row.
pub enum Mapped {
    Item(Value),
    /// Skipped: a category for the report ("unsupported row", ...).
    Skip(&'static str),
}

/// The CVV columns, never read (§10.2).
const CVV: [&str; 4] = ["cvv", "cvc", "security_code", "code"];

impl Dialect {
    pub fn detect(header: &[String]) -> Option<Dialect> {
        let cols: Vec<String> = header.iter().map(|h| h.trim().to_ascii_lowercase()).collect();
        let has = |n: &str| cols.iter().any(|c| c == n);
        let kind = if has("password") && (has("username") || has("login") || has("url") || has("website")) {
            Kind::Login
        } else if has("card_number") || has("cc_number") || has("number") && (has("expiration") || has("expiry")) {
            Kind::Card
        } else if (has("note") || has("content") || has("text")) && (has("title") || has("name")) {
            Kind::Note
        } else {
            return None;
        };
        Some(Dialect { kind, cols })
    }

    fn get<'a>(&self, row: &'a [String], names: &[&str]) -> Option<&'a str> {
        names.iter().find_map(|n| {
            let i = self.cols.iter().position(|c| c == n)?;
            row.get(i).map(|s| s.trim()).filter(|s| !s.is_empty())
        })
    }

    /// A secret value exactly as exported: passwords and note bodies keep
    /// their spaces (final review VER-I5).
    fn raw<'a>(&self, row: &'a [String], names: &[&str]) -> Option<&'a str> {
        names.iter().find_map(|n| {
            let i = self.cols.iter().position(|c| c == n)?;
            row.get(i).map(String::as_str).filter(|s| !s.is_empty())
        })
    }

    pub fn map(&self, row: &[String]) -> Mapped {
        if row.len() != self.cols.len() {
            return Mapped::Skip("column count mismatch");
        }
        match self.kind {
            Kind::Login => self.login(row),
            Kind::Card => self.card(row),
            Kind::Note => self.note(row),
        }
    }

    /// Whether this row carries an OTP secret (reported, not imported).
    pub fn has_otp(&self, row: &[String]) -> bool {
        self.kind == Kind::Login && self.get(row, &["otpsecret", "otpurl"]).is_some()
    }

    fn login(&self, row: &[String]) -> Mapped {
        let Some(password) = self.raw(row, &["password"]) else { return Mapped::Skip("no password") };
        let username = self.get(row, &["username", "login", "username2", "username3"]).unwrap_or("");
        let site = self.get(row, &["url", "website"]).and_then(host_of);
        let title = self.get(row, &["title", "name"]).map(nfc).or_else(|| site.as_ref().map(|(h, _)| h.clone())).unwrap_or_else(|| "Imported login".into());
        let urls: Vec<Value> = site.into_iter().map(|(h, http)| json!({ "host": h, "match": "exact", "allow_http": http })).collect();
        Mapped::Item(json!({
            "kind": "login", "title": title, "username": username, "password": password, "urls": urls,
            "notes": self.get(row, &["note"]).unwrap_or(""),
        }))
    }

    fn card(&self, row: &[String]) -> Mapped {
        let Some(number) = self.get(row, &["card_number", "cc_number", "number"]) else { return Mapped::Skip("no card number") };
        let split = match (self.get(row, &["expiration_month"]), self.get(row, &["expiration_year"])) {
            (Some(m), Some(y)) => Some(format!("{m}/{y}")),
            _ => None,
        };
        let Some(expiry) = self.get(row, &["expiration", "expiry"]).map(String::from).or(split).as_deref().and_then(expiry) else {
            return Mapped::Skip("unreadable expiry");
        };
        let mut item = json!({
            "kind": "card", "label": self.get(row, &["name", "account_name"]).map_or_else(|| "Imported card".into(), nfc), "number": number, "expiry": expiry,
            "cardholder": self.get(row, &["cardholder", "name_on_card", "account_holder"]).unwrap_or(""),
        });
        let billing = |k: &str| self.get(row, &[&format!("billing_{k}")]).unwrap_or("").to_string();
        let addr = json!({"line1": billing("address"), "line2": billing("address2"), "city": billing("city"), "region": billing("state"), "postal": billing("zip"), "country": billing("country")});
        if addr.as_object().is_some_and(|o| o.values().any(|v| v != "")) {
            item["billing_address"] = addr;
        }
        Mapped::Item(item)
    }

    fn note(&self, row: &[String]) -> Mapped {
        let Some(title) = self.get(row, &["title", "name"]).map(nfc) else { return Mapped::Skip("no title") };
        Mapped::Item(json!({ "kind": "secure_note", "title": title, "body": self.raw(row, &["note", "content", "text"]).unwrap_or("") }))
    }

    /// The row's CVV column, if the export had one (for the report only).
    pub fn has_cvv(&self) -> bool {
        self.kind == Kind::Card && self.cols.iter().any(|c| CVV.contains(&c.as_str()))
    }
}

/// `https://login.example.com/path` → `login.example.com` (§9.4 rules
/// apply when the item is validated); a bare host is taken as is. A plain
/// `http://` URL keeps working only for a local host (router pages and the
/// like, §9.5): `allow_http` (final review VER-O4).
fn host_of(url: &str) -> Option<(String, bool)> {
    let full = if url.contains("://") { url.to_string() } else { format!("https://{url}") };
    let o = crate::browser::origin::canonical(&full).ok()?;
    let http = o.scheme == "http" && o.is_local();
    Some((o.host, http))
}

/// Display text in NFC, as records require (§10.3; final review VER-O4).
fn nfc(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfc().collect()
}

/// MM/YY from `MM/YY`, `MM/YYYY`, `YYYY-MM` or `MM-YYYY`.
fn expiry(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.split(['/', '-', '.']).map(str::trim).collect();
    let (m, y) = match parts.as_slice() {
        [a, b] if a.len() == 4 => (*b, *a),
        [a, b] => (*a, *b),
        _ => return None,
    };
    let m: u8 = m.parse().ok().filter(|m| (1..=12).contains(m))?;
    let y = match y.len() {
        2 => y.to_string(),
        4 => y[2..].to_string(),
        _ => return None,
    };
    y.chars().all(|c| c.is_ascii_digit()).then(|| format!("{m:02}/{y}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn dialects_and_mapping() {
        let d = Dialect::detect(&s(&["Username", "Title", "Password", "Note", "URL", "Category", "otpSecret"])).unwrap();
        assert_eq!(d.kind, Kind::Login);
        let Mapped::Item(v) = d.map(&s(&["alice@example.test", "Example", "fixture-pw-1", "", "https://login.example.test/x", "", ""])) else { panic!() };
        assert_eq!(v["urls"][0]["host"], "login.example.test");
        let Mapped::Item(v) = d.map(&s(&["a", "Cafe\u{301}", "p", "", "http://192.168.1.1/", "", ""])) else { panic!() };
        assert_eq!((v["title"].as_str(), v["urls"][0]["allow_http"].as_bool()), (Some("Caf\u{e9}"), Some(true)), "NFC title; local http kept");
        let Mapped::Item(v) = d.map(&s(&["a", "T", " fixture spaced ", "", "", "", ""])) else { panic!() };
        assert_eq!(v["password"], " fixture spaced ", "a password keeps its spaces");
        assert!(!d.has_otp(&s(&["a", "b", "c", "", "", "", ""])));
        assert!(d.has_otp(&s(&["a", "b", "c", "", "", "", "JBSWY3DPEHPK3PXP"])));
        let c = Dialect::detect(&s(&["name", "card_number", "expiration", "cardholder", "cvv"])).unwrap();
        assert!(c.has_cvv());
        let Mapped::Item(v) = c.map(&s(&["Visa", "4111111111111111", "2031-07", "A Test", "123"])) else { panic!() };
        assert_eq!(v["expiry"], "07/31");
        assert!(!v.to_string().contains("123"), "the CVV is never read");
        let c = Dialect::detect(&s(&["account_name", "cc_number", "code", "expiration_month", "expiration_year", "billing_city"])).unwrap();
        let Mapped::Item(v) = c.map(&s(&["Card", "4111111111111111", "999", "3", "2030", "Testville"])) else { panic!() };
        assert_eq!((v["expiry"].as_str(), v["billing_address"]["city"].as_str()), (Some("03/30"), Some("Testville")));
        assert!(c.has_cvv() && !v.to_string().contains("999"));
        let n = Dialect::detect(&s(&["title", "note"])).unwrap();
        assert_eq!(n.kind, Kind::Note);
        assert!(Dialect::detect(&s(&["foo", "bar"])).is_none());
        assert!(matches!(d.map(&s(&["only"])), Mapped::Skip(_)));
    }
}
