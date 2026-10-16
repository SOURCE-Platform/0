//! Origin canonicalization (spec §9.4; Phase H): what the helper compares
//! is `scheme://host[:port]` — `http`/`https` only, IDN hosts in UTS-46
//! punycode (`xn--`), lowercase, no trailing dot, no default port, no
//! userinfo, no path. The UTS-46 mapping itself is the browser's: Chrome
//! reports `location.origin` and `tab.url` already in punycode, and the
//! app converts a typed host before saving. The helper accepts ASCII
//! letter-digit-hyphen labels only and refuses any other host (no IDN
//! library in the helper's graph, §17.4). Forms that hide the real host are refused (§9.5):
//! userinfo, IP-literal tricks (decimal, octal, hex, short dotted forms),
//! dotless names other than `localhost`. Fail closed: anything unclear is
//! an error, never a guess.

use crate::errors::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
}

impl Origin {
    pub fn text(&self) -> String {
        match self.port {
            Some(p) => format!("{}://{}:{}", self.scheme, self.host, p),
            None => format!("{}://{}", self.scheme, self.host),
        }
    }

    /// A host the §9.5 http exception may cover: localhost, 127.0.0.1, RFC 1918.
    pub fn is_local(&self) -> bool {
        if self.host == "localhost" {
            return true;
        }
        match self.host.parse::<std::net::Ipv4Addr>() {
            Ok(a) => a.is_loopback() || a.is_private(),
            Err(_) => false,
        }
    }

    /// The host was IDN (punycode) — §9.5's look-alike warning applies.
    pub fn is_punycode(&self) -> bool {
        self.host.split('.').any(|l| l.starts_with("xn--"))
    }
}

fn refuse() -> ErrorCode {
    ErrorCode::InvalidInput
}

/// Canonicalize a page origin or a full URL (the path is dropped).
pub fn canonical(input: &str) -> Result<Origin, ErrorCode> {
    let (scheme, rest) = input.split_once("://").ok_or_else(refuse)?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(refuse()); // data:, file:, about:, opaque origins
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return Err(refuse());
    }
    let (host_raw, port) = split_port(authority)?;
    let host = canonical_host(host_raw)?;
    let default = if scheme == "https" { 443 } else { 80 };
    Ok(Origin { scheme, host, port: port.filter(|p| *p != default) })
}

fn split_port(authority: &str) -> Result<(&str, Option<u16>), ErrorCode> {
    if authority.starts_with('[') {
        return Err(refuse()); // IPv6 literals are not stored-site forms
    }
    match authority.rsplit_once(':') {
        Some((h, p)) => {
            let port = p.parse::<u16>().ok().filter(|p| *p != 0 && !p.to_string().is_empty()).ok_or_else(refuse)?;
            if p.starts_with('0') || p.starts_with('+') {
                return Err(refuse());
            }
            Ok((h, Some(port)))
        }
        None => Ok((authority, None)),
    }
}

fn canonical_host(raw: &str) -> Result<String, ErrorCode> {
    let trimmed = raw.strip_suffix('.').unwrap_or(raw);
    if trimmed.is_empty() || trimmed.ends_with('.') {
        return Err(refuse());
    }
    if !trimmed.is_ascii() {
        return Err(refuse()); // Unicode: the browser sends punycode
    }
    let ascii = trimmed.to_ascii_lowercase();
    let ldh = |l: &str| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ascii.len() > 253 || !ascii.split('.').all(ldh) {
        return Err(refuse());
    }
    if ascii == "localhost" {
        return Ok(ascii);
    }
    // A host made only of digits, dots and hex/octal markers is an IP form:
    // only a plain dotted quad with canonical octets is accepted.
    let numeric = ascii.split('.').all(|l| !l.is_empty() && (l.chars().all(|c| c.is_ascii_digit()) || l.starts_with("0x")));
    if numeric {
        let octets: Vec<&str> = ascii.split('.').collect();
        let canonical_quad = octets.len() == 4 && octets.iter().all(|o| o.parse::<u8>().is_ok() && (o.len() == 1 || !o.starts_with('0')));
        return if canonical_quad { Ok(ascii) } else { Err(refuse()) };
    }
    if !ascii.contains('.') {
        return Err(refuse()); // dotless names other than localhost
    }
    Ok(ascii)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> Result<String, ErrorCode> {
        canonical(s).map(|o| o.text())
    }

    #[test]
    fn canonical_forms() {
        assert_eq!(c("https://GitHub.com/login?x=1").unwrap(), "https://github.com");
        assert_eq!(c("https://github.com:443").unwrap(), "https://github.com");
        assert_eq!(c("http://github.com:80/").unwrap(), "http://github.com");
        assert_eq!(c("https://github.com.:8443").unwrap(), "https://github.com:8443");
        assert_eq!(c("https://XN--BCHER-KVA.example").unwrap(), "https://xn--bcher-kva.example");
        assert!(c("https://bücher.example").is_err(), "Unicode hosts arrive as punycode");
        assert!(c("https://exa_mple.com").is_err() && c("https://-a.example.com").is_err());
        assert_eq!(c("http://localhost:3000").unwrap(), "http://localhost:3000");
        assert_eq!(c("http://192.168.1.4").unwrap(), "http://192.168.1.4");
    }

    #[test]
    fn hidden_hosts_are_refused() {
        for bad in [
            "https://user@github.com", "https://github.com@evil.test", "file:///etc/passwd", "data:text/html,x",
            "about:blank", "https://3232235777", "https://0x7f.0.0.1", "https://010.0.0.1", "https://127.1",
            "https://intranet", "https://[::1]", "javascript:alert(1)", "https://", "https://github.com:0",
        ] {
            assert!(canonical(bad).is_err(), "{bad}");
        }
    }
}
