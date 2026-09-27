//! Service configuration from the environment. The only provider-held
//! secret is the locate pepper (§11.1), which carries no vault authority.
//!
//! - `PROVIDER_ORIGIN`  this service's public origin, e.g. `https://vault.example.com`
//! - `PROVIDER_PEPPER`  64 hex chars (32 bytes), from the host's secret store
//! - `PROVIDER_PORT`    listen port (default 8787)
//! - `PROVIDER_STORE`   `s3` or `fs:/path` (rehearsals and tests)
//! - for `s3`: `S3_BUCKET`, `AWS_REGION`, `AWS_ACCESS_KEY_ID`,
//!   `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`, optional
//!   `S3_ENDPOINT` (defaults to the regional AWS endpoint)

use vault_proto::crypto::hex;

pub enum Store {
    Fs(String),
    S3(crate::s3::S3Config),
}

pub struct Config {
    pub origin: String,
    pub pepper: [u8; 32],
    pub port: u16,
    pub store: Store,
}

fn var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is not set"))
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        let origin = var("PROVIDER_ORIGIN")?;
        if !vault_proto::request::valid_audience(&origin) {
            return Err("PROVIDER_ORIGIN must be a lowercase https://host[:port] origin".into());
        }
        let pepper = hex::decode_array::<32>(&var("PROVIDER_PEPPER")?).ok_or("PROVIDER_PEPPER must be 64 hex characters")?;
        let port = std::env::var("PROVIDER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8787);
        let store = match var("PROVIDER_STORE")?.as_str() {
            "s3" => Store::S3(crate::s3::S3Config::from_env()?),
            s if s.starts_with("fs:") => Store::Fs(s[3..].to_string()),
            _ => return Err("PROVIDER_STORE must be s3 or fs:/path".into()),
        };
        Ok(Config { origin, pepper, port, store })
    }
}
