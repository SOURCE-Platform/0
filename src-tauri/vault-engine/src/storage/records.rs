//! Record plaintext models (spec §8.1 login, §8.2 card) and the §8
//! validation rules enforced at the add/update boundary. Serde uses
//! `deny_unknown_fields` **off** for forward compatibility (§8); the one
//! hard exception is unknown secret-class fields, which the ops layer
//! rejects outright (§8.2: no code path may ever store a `cvv`).

use serde::{Deserialize, Serialize};

use crate::errors::ErrorCode;

pub const KIND_LOGIN: u8 = 1;
pub const KIND_CARD: u8 = 2;

pub fn kind_name(kind_tag: u8) -> &'static str {
    match kind_tag {
        KIND_LOGIN => "login",
        KIND_CARD => "card",
        _ => "unknown",
    }
}

pub fn kind_tag(name: &str) -> Option<u8> {
    match name {
        "login" => Some(KIND_LOGIN),
        "card" => Some(KIND_CARD),
        _ => None,
    }
}

pub const SCHEMA_LOGIN: &str = "login@1";
pub const SCHEMA_CARD: &str = "card@1";

const TITLE_MAX: usize = 200;
const LABEL_MAX: usize = 200;
const USERNAME_MAX: usize = 320;
const PASSWORD_MAX: usize = 1024;
const NOTES_MAX: usize = 10_000;
const URLS_MAX: usize = 20;
const HISTORY_MAX: usize = 10;

fn default_match() -> String {
    "exact".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub password: String,
    pub changed_at: u64,
}

/// §8.1 plaintext shape (lives only inside `ct`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRecord {
    pub schema: String,
    pub title: String,
    #[serde(default)]
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub urls: Vec<UrlEntryWire>,
    #[serde(default)]
    pub notes: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub password_history: Vec<HistoryEntry>,
}

/// Wire form of a URL entry with the literal `match` key (§8.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlEntryWire {
    pub host: String,
    #[serde(rename = "match", default = "default_match")]
    pub match_mode: String,
    #[serde(default)]
    pub allow_http: bool,
}

/// §8.2 plaintext shape. There is intentionally no `cvv` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardRecord {
    pub schema: String,
    pub label: String,
    pub number: String,
    pub expiry: String,
    pub cardholder: String,
    #[serde(default)]
    pub billing_address: Option<BillingAddress>,
    #[serde(default)]
    pub notes: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingAddress {
    #[serde(default)]
    pub line1: String,
    #[serde(default)]
    pub line2: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub postal: String,
    #[serde(default)]
    pub country: String,
}

/// §8.2: reject unknown secret-class fields at the IPC boundary. The
/// list is deliberately generic — a `cvv` key is refused on any kind.
pub fn reject_forbidden_fields(input: &serde_json::Value) -> Result<(), ErrorCode> {
    const FORBIDDEN: [&str; 3] = ["cvv", "cvc", "security_code"];
    if let Some(obj) = input.as_object() {
        for key in obj.keys() {
            if FORBIDDEN.contains(&key.as_str()) {
                return Err(ErrorCode::InvalidInput);
            }
        }
    }
    Ok(())
}

fn check_len(field: &str, max: usize) -> Result<(), ErrorCode> {
    if field.chars().count() > max {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

/// Host shape per §8.1: canonical registrable-or-exact host, never a
/// full URL with path/query. Lowercased in place.
pub fn normalize_host(host: &str) -> Result<String, ErrorCode> {
    let h = host.trim().to_ascii_lowercase();
    let bad = h.is_empty()
        || h.len() > 253
        || h.contains(['/', ':', '?', '#', '@', ' '])
        || h.starts_with('.')
        || h.ends_with('.')
        || !h
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if bad {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(h)
}

fn validate_urls(urls: &[UrlEntryWire]) -> Result<Vec<UrlEntryWire>, ErrorCode> {
    if urls.is_empty() || urls.len() > URLS_MAX {
        return Err(ErrorCode::InvalidInput);
    }
    let mut out = Vec::with_capacity(urls.len());
    for u in urls {
        if u.match_mode != "exact" && u.match_mode != "domain" {
            return Err(ErrorCode::InvalidInput);
        }
        out.push(UrlEntryWire {
            host: normalize_host(&u.host)?,
            match_mode: u.match_mode.clone(),
            allow_http: u.allow_http,
        });
    }
    Ok(out)
}

/// Validate + normalize a login record in place (§8.1 rules).
pub fn validate_login(rec: &mut LoginRecord) -> Result<(), ErrorCode> {
    if rec.schema != SCHEMA_LOGIN {
        return Err(ErrorCode::InvalidInput);
    }
    if rec.title.is_empty() || !unicode_normalization::is_nfc(rec.title.as_str()) {
        return Err(ErrorCode::InvalidInput);
    }
    check_len(&rec.title, TITLE_MAX)?;
    check_len(&rec.username, USERNAME_MAX)?;
    if rec.password.is_empty() {
        return Err(ErrorCode::InvalidInput);
    }
    check_len(&rec.password, PASSWORD_MAX)?;
    check_len(&rec.notes, NOTES_MAX)?;
    rec.urls = validate_urls(&rec.urls)?;
    if rec.password_history.len() > HISTORY_MAX {
        let overflow = rec.password_history.len() - HISTORY_MAX;
        rec.password_history.drain(0..overflow); // oldest evicted (§8.1)
    }
    Ok(())
}

/// Validate + normalize a card record (§8.2). Returns `true` when the
/// number fails the Luhn check — a UX warning, not a hard error (§8.2).
pub fn validate_card(rec: &mut CardRecord) -> Result<bool, ErrorCode> {
    if rec.schema != SCHEMA_CARD {
        return Err(ErrorCode::InvalidInput);
    }
    if rec.label.is_empty() || !unicode_normalization::is_nfc(rec.label.as_str()) {
        return Err(ErrorCode::InvalidInput);
    }
    check_len(&rec.label, LABEL_MAX)?;
    check_len(&rec.notes, NOTES_MAX)?;
    let digits: String = rec.number.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 12 || digits.len() > 19 {
        return Err(ErrorCode::InvalidInput);
    }
    rec.number = digits;
    let expiry_ok = rec.expiry.len() == 5
        && rec.expiry.as_bytes()[2] == b'/'
        && rec
            .expiry
            .chars()
            .enumerate()
            .all(|(i, c)| i == 2 || c.is_ascii_digit());
    if !expiry_ok {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(!luhn_ok(&rec.number))
}

fn luhn_ok(digits: &str) -> bool {
    let mut sum = 0u32;
    for (i, c) in digits.chars().rev().enumerate() {
        let mut d = c.to_digit(10).unwrap_or(0);
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    sum % 10 == 0
}

/// Metadata JSON sealed into `meta_ct` (§3.4: title/username/hosts are
/// metadata-class; everything else stays in `ct`).
pub fn meta_json(kind: u8, title: &str, username: &str, hosts: &[String]) -> serde_json::Value {
    let _ = kind;
    serde_json::json!({
        "title": title,
        "username": username,
        "hosts": hosts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn login() -> LoginRecord {
        LoginRecord {
            schema: SCHEMA_LOGIN.to_string(),
            title: "GitHub".to_string(),
            username: "alice@example.com".to_string(),
            password: "synthetic".to_string(),
            urls: vec![UrlEntryWire {
                host: "GitHub.com".to_string(),
                match_mode: "exact".to_string(),
                allow_http: false,
            }],
            notes: String::new(),
            created_at: 1,
            updated_at: 1,
            password_history: vec![],
        }
    }

    #[test]
    fn login_validation_normalizes_host() {
        let mut rec = login();
        validate_login(&mut rec).unwrap();
        assert_eq!(rec.urls[0].host, "github.com");
    }

    #[test]
    fn login_validation_rejects_bad_fields() {
        let cases: Vec<fn(&mut LoginRecord)> = vec![
            |r| r.title = String::new(),
            |r| r.title = "x".repeat(201),
            |r| r.password = String::new(),
            |r| r.password = "x".repeat(1025),
            |r| r.urls = vec![],
            |r| r.urls[0].host = "https://github.com/login".to_string(),
            |r| r.urls[0].match_mode = "fuzzy".to_string(),
            |r| r.notes = "x".repeat(10_001),
        ];
        for (i, mutate) in cases.iter().enumerate() {
            let mut rec = login();
            mutate(&mut rec);
            assert_eq!(validate_login(&mut rec), Err(ErrorCode::InvalidInput), "case {i}");
        }
    }

    #[test]
    fn history_is_capped_at_ten() {
        let mut rec = login();
        rec.password_history = (0..15)
            .map(|i| HistoryEntry {
                password: format!("p{i}"),
                changed_at: i,
            })
            .collect();
        validate_login(&mut rec).unwrap();
        assert_eq!(rec.password_history.len(), 10);
        assert_eq!(rec.password_history[0].password, "p5"); // oldest evicted
    }

    #[test]
    fn cvv_is_rejected_outright() {
        assert_eq!(
            reject_forbidden_fields(&json!({"label": "x", "cvv": "123"})),
            Err(ErrorCode::InvalidInput)
        );
        assert!(reject_forbidden_fields(&json!({"label": "x"})).is_ok());
    }

    #[test]
    fn card_luhn_is_warning_not_error() {
        let mut card = CardRecord {
            schema: SCHEMA_CARD.to_string(),
            label: "Personal Visa".to_string(),
            number: "4111 1111 1111 1111".to_string(),
            expiry: "09/28".to_string(),
            cardholder: "A EXAMPLE".to_string(),
            billing_address: None,
            notes: String::new(),
            created_at: 1,
            updated_at: 1,
        };
        let warn = validate_card(&mut card).unwrap();
        assert!(!warn, "canonical test number passes Luhn");
        assert_eq!(card.number, "4111111111111111");
        card.number = "4111111111111112".to_string();
        let warn = validate_card(&mut card).unwrap();
        assert!(warn, "bad check digit warns but allows");
        card.expiry = "9/28".to_string();
        assert_eq!(validate_card(&mut card), Err(ErrorCode::InvalidInput));
    }

    #[test]
    fn non_nfc_title_rejected() {
        let mut rec = login();
        rec.title = "e\u{301}".to_string(); // e + combining acute (NFD)
        assert_eq!(validate_login(&mut rec), Err(ErrorCode::InvalidInput));
        rec.title = "é".to_string(); // precomposed (NFC)
        validate_login(&mut rec).unwrap();
    }
}
