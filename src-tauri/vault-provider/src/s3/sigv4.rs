//! AWS Signature Version 4 for S3 (header-based, `UNSIGNED-PAYLOAD` is
//! never used: the payload hash is always the SHA-256 of the body).
//! Written from the published algorithm; tested against an independent
//! Python implementation's output.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use vault_proto::crypto::hex;

pub struct Credentials {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("any key length");
    m.update(data);
    m.finalize().into_bytes().into()
}

/// RFC 3986 unreserved characters stay; everything else is %XX; `/` is
/// kept in paths.
pub fn uri_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') || (keep_slash && b == b'/');
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Sign one request. `headers` are the extra headers to sign (lowercase
/// names); returns every header to send, `authorization` included.
#[allow(clippy::too_many_arguments)]
pub fn sign(
    creds: &Credentials,
    region: &str,
    method: &str,
    host: &str,
    path: &str,
    query: &[(String, String)],
    headers: &[(String, String)],
    body: &[u8],
    amz_date: &str,
) -> Vec<(String, String)> {
    let date = &amz_date[..8];
    let payload = hex::encode(Sha256::digest(body));
    let mut all: Vec<(String, String)> = headers.to_vec();
    all.push(("host".into(), host.into()));
    all.push(("x-amz-content-sha256".into(), payload.clone()));
    all.push(("x-amz-date".into(), amz_date.into()));
    if let Some(t) = &creds.session_token {
        all.push(("x-amz-security-token".into(), t.clone()));
    }
    all.sort();
    let mut q: Vec<(String, String)> = query.iter().map(|(k, v)| (uri_encode(k, false), uri_encode(v, false))).collect();
    q.sort();
    let canonical_query = q.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
    let canonical_headers: String = all.iter().map(|(k, v)| format!("{k}:{}\n", v.trim())).collect();
    let signed = all.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    let canonical = format!("{method}\n{}\n{canonical_query}\n{canonical_headers}\n{signed}\n{payload}", uri_encode(path, true));
    let scope = format!("{date}/{region}/s3/aws4_request");
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", hex::encode(Sha256::digest(canonical.as_bytes())));
    let k = hmac(format!("AWS4{}", creds.secret_key).as_bytes(), date.as_bytes());
    let k = hmac(&k, region.as_bytes());
    let k = hmac(&k, b"s3");
    let k = hmac(&k, b"aws4_request");
    let signature = hex::encode(hmac(&k, to_sign.as_bytes()));
    all.push((
        "authorization".into(),
        format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}", creds.access_key),
    ));
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected values come from an independent Python implementation of
    /// SigV4 (hashlib/hmac, written separately from this file) over the
    /// same synthetic inputs.
    #[test]
    fn matches_reference() {
        let creds = Credentials { access_key: "SYNTHETICACCESSKEY".into(), secret_key: "synthetic-secret-for-tests".into(), session_token: None };
        let h = sign(
            &creds,
            "eu-west-1",
            "PUT",
            "bucket.s3.eu-west-1.amazonaws.com",
            "/v2/vaults/a0/state",
            &[],
            &[("if-none-match".into(), "*".into())],
            b"synthetic body",
            "20260927T151518Z",
        );
        let auth = h.iter().find(|(k, _)| k == "authorization").unwrap().1.clone();
        assert!(auth.ends_with(include_str!("../../tests/sigv4_expected.txt").trim()), "{auth}");
    }
}
