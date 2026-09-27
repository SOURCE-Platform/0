//! Provider HTTPS transport (spec v0.4 §11.1): Apple's URLSession with the
//! system's standard certificate validation — no pinning, no custom trust.
//! Carries only what the helper signed and the ciphertext main moves; the
//! `Ov0-Auth` header is single-use (nonce + ±300 s window).

use vault_coordinator::{HttpResponse, Transport, TransportError};

pub struct ProviderHttp;

const TIMEOUT_SECS: f64 = 30.0;

#[cfg(target_os = "macos")]
impl Transport for ProviderHttp {
    fn send(&self, origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        use std::sync::mpsc;

        use block2::RcBlock;
        use objc2_foundation::{NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSString, NSURLResponse, NSURLSession, NSURL};

        let unreachable = |why: &str| TransportError::Unreachable(why.to_string());
        let url = NSURL::URLWithString(&NSString::from_str(&format!("{origin}{path}"))).ok_or_else(|| unreachable("bad url"))?;
        let req = NSMutableURLRequest::requestWithURL(&url);
        req.setHTTPMethod(&NSString::from_str(method));
        req.setTimeoutInterval(TIMEOUT_SECS);
        if let Some(a) = auth {
            req.setValue_forHTTPHeaderField(Some(&NSString::from_str(a)), &NSString::from_str("Ov0-Auth"));
        }
        if !body.is_empty() {
            req.setHTTPBody(Some(&NSData::with_bytes(body)));
            req.setValue_forHTTPHeaderField(Some(&NSString::from_str("application/octet-stream")), &NSString::from_str("Content-Type"));
        }
        let (tx, rx) = mpsc::channel::<Result<HttpResponse, TransportError>>();
        let handler = RcBlock::new(move |data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError| {
            let out = if !error.is_null() || response.is_null() {
                Err(TransportError::Unreachable("request failed".into()))
            } else {
                // SAFETY: URLSession hands a valid response for HTTP(S) URLs;
                // it is an NSHTTPURLResponse for http/https schemes.
                let http = unsafe { &*(response as *const NSHTTPURLResponse) };
                let date = http.valueForHTTPHeaderField(&NSString::from_str("Date")).and_then(|d| parse_http_date(&d.to_string()));
                // SAFETY: `data` is either null or a valid NSData for the call.
                let body = if data.is_null() { Vec::new() } else { unsafe { &*data }.to_vec() };
                Ok(HttpResponse { status: http.statusCode() as u16, body, date })
            };
            let _ = tx.send(out);
        });
        let session = NSURLSession::sharedSession();
        // SAFETY: the completion block only sends on a channel (Send).
        let task = unsafe { session.dataTaskWithRequest_completionHandler(&req, &handler) };
        task.resume();
        rx.recv_timeout(std::time::Duration::from_secs(TIMEOUT_SECS as u64 + 5)).map_err(|_| unreachable("timeout"))?
    }
}

#[cfg(not(target_os = "macos"))]
impl Transport for ProviderHttp {
    fn send(&self, _: &str, _: &str, _: &str, _: Option<&str>, _: &[u8]) -> Result<HttpResponse, TransportError> {
        Err(TransportError::Unreachable("vault backup is macOS-only".into()))
    }
}

/// RFC 7231 IMF-fixdate (`Sun, 27 Sep 2026 15:15:18 GMT`) → Unix seconds.
pub fn parse_http_date(s: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc2822(s).ok().and_then(|d| u64::try_from(d.timestamp()).ok())
}

#[cfg(test)]
mod tests {
    #[test]
    fn http_date() {
        assert_eq!(super::parse_http_date("Sun, 27 Sep 2026 15:15:18 GMT"), Some(1_790_522_118));
        assert_eq!(super::parse_http_date("not a date"), None);
    }
}
