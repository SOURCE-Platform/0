//! A lean S3 REST client: GET/PUT/DELETE with conditional headers and
//! ListObjectsV2, signed with SigV4. Virtual-hosted-style URLs.

use super::sigv4::{sign, uri_encode, Credentials};

pub struct S3Config {
    pub region: String,
    pub endpoint: String,
    pub creds: Credentials,
}

impl S3Config {
    pub fn from_env() -> Result<S3Config, String> {
        let v = |n: &str| std::env::var(n).map_err(|_| format!("{n} is not set"));
        let bucket = v("S3_BUCKET")?;
        let region = v("AWS_REGION")?;
        let endpoint = std::env::var("S3_ENDPOINT").unwrap_or_else(|_| format!("https://{bucket}.s3.{region}.amazonaws.com"));
        let creds = Credentials { access_key: v("AWS_ACCESS_KEY_ID")?, secret_key: v("AWS_SECRET_ACCESS_KEY")?, session_token: std::env::var("AWS_SESSION_TOKEN").ok() };
        Ok(S3Config { region, endpoint, creds })
    }
}

pub struct S3Client {
    cfg: S3Config,
    http: reqwest::blocking::Client,
}

pub struct S3Response {
    pub status: u16,
    pub etag: Option<String>,
    /// `Content-Length` (the object size for a HEAD).
    pub length: Option<u64>,
    pub body: Vec<u8>,
}

fn amz_now() -> String {
    // UTC basic format YYYYMMDDTHHMMSSZ from the system clock.
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Howard Hinnant's days → civil date.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

impl S3Client {
    pub fn new(cfg: S3Config) -> S3Client {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(20)).build().expect("TLS client");
        S3Client { cfg, http }
    }

    pub fn request(&self, method: &str, key: &str, query: &[(String, String)], headers: &[(String, String)], body: &[u8]) -> Result<S3Response, ()> {
        let host = self.cfg.endpoint.trim_start_matches("https://").trim_start_matches("http://").to_string();
        let path = format!("/{key}");
        let signed = sign(&self.cfg.creds, &self.cfg.region, method, &host, &path, query, headers, body, &amz_now());
        let qs = query.iter().map(|(k, v)| format!("{}={}", uri_encode(k, false), uri_encode(v, false))).collect::<Vec<_>>().join("&");
        let url = format!("{}{}{}{}", self.cfg.endpoint, uri_encode(&path, true), if qs.is_empty() { "" } else { "?" }, qs);
        let mut rb = self.http.request(method.parse().map_err(|_| ())?, url);
        for (k, v) in signed.iter().filter(|(k, _)| k != "host") {
            rb = rb.header(k, v);
        }
        let r = rb.body(body.to_vec()).send().map_err(|_| ())?;
        let status = r.status().as_u16();
        let etag = r.headers().get("etag").and_then(|v| v.to_str().ok()).map(String::from);
        let length = r.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
        let body = r.bytes().map_err(|_| ())?.to_vec();
        Ok(S3Response { status, etag, length, body })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn civil_dates() {
        assert_eq!(super::civil_from_days(0), (1970, 1, 1));
        assert_eq!(super::civil_from_days(20_723), (2026, 9, 27));
    }
}
