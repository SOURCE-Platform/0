use crate::{secrets::Credentials, Error, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::{Client, Method, Url};
use rsa::{
    pkcs1::DecodeRsaPrivateKey,
    pkcs1v15::SigningKey,
    pkcs8::DecodePrivateKey,
    signature::{RandomizedSigner, SignatureEncoding},
    traits::PublicKeyParts,
    RsaPrivateKey,
};
use serde_json::{json, Value};
use sha2::Sha256;
use std::time::Duration;

const API: &str = "https://api.enablebanking.com";
const MAX_RESPONSE: usize = 8 * 1024 * 1024;

pub struct EnableBanking {
    http: Client,
    signing_key: SigningKey<Sha256>,
    application_id: String,
}

pub fn signing_key(pem: &str) -> Result<SigningKey<Sha256>> {
    let key = RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
        .map_err(|_| Error::InvalidKey)?;
    if key.n().bits() < 2048 || key.n().bits() > 8192 {
        return Err(Error::InvalidKey);
    }
    key.validate().map_err(|_| Error::InvalidKey)?;
    Ok(SigningKey::new(key))
}

impl EnableBanking {
    pub fn new(credentials: &Credentials) -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15))
            .user_agent("SOURCE-personal-banking/0.1")
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Self {
            http,
            signing_key: signing_key(&credentials.private_key)?,
            application_id: credentials.application_id.clone(),
        })
    }

    pub(crate) fn jwt(&self) -> Result<String> {
        let now = chrono::Utc::now().timestamp();
        let header = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "typ":"JWT", "alg":"RS256", "kid": self.application_id,
            }))
            .map_err(|_| Error::InvalidKey)?,
        );
        let body = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "iss":"enablebanking.com", "aud":"api.enablebanking.com", "iat":now, "exp":now+300,
            }))
            .map_err(|_| Error::InvalidKey)?,
        );
        let input = format!("{header}.{body}");
        let signature = self
            .signing_key
            .sign_with_rng(&mut rsa::rand_core::OsRng, input.as_bytes());
        Ok(format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        ))
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value> {
        let mut request = self
            .http
            .request(method, format!("{API}{path}"))
            .bearer_auth(self.jwt()?)
            .query(query)
            .header("Accept", "application/json");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request.send().await.map_err(|_| Error::Network)?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE as u64)
        {
            return Err(Error::InvalidRecord);
        }
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Network)? {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err(Error::InvalidRecord);
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
            if status.is_success() {
                Error::InvalidRecord
            } else {
                Error::Provider(status.as_u16())
            }
        })?;
        if !status.is_success() {
            return Err(match value["error"].as_str().unwrap_or("") {
                "EXPIRED_SESSION" => Error::Expired,
                "PSU_HEADER_NOT_PROVIDED" | "PSUHEADER_NOT_PROVIDED" | "PSUHEADERNOT_PROVIDED" => {
                    Error::UserHeaders
                }
                _ if status.as_u16() == 429 => Error::RateLimited,
                _ => Error::Provider(status.as_u16()),
            });
        }
        Ok(value)
    }

    pub async fn millennium(&self) -> Result<Value> {
        let data = self
            .request(
                Method::GET,
                "/aspsps",
                &[
                    ("country", "PL".into()),
                    ("psu_type", "personal".into()),
                    ("service", "AIS".into()),
                ],
                None,
            )
            .await?;
        let banks = data["aspsps"].as_array().ok_or(Error::InvalidRecord)?;
        let mut matches = banks.iter().filter(|b| {
            b["country"] == "PL"
                && b["name"]
                    .as_str()
                    .is_some_and(|n| n.to_lowercase().contains("millennium"))
                && b["psu_types"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|t| t == "personal"))
        });
        let bank = matches.next().ok_or(Error::BankUnavailable)?.clone();
        if matches.next().is_some() {
            return Err(Error::BankUnavailable);
        }
        Ok(bank)
    }

    pub async fn start_auth(&self, bank: &Value, state: &str) -> Result<String> {
        let seconds = bank["maximum_consent_validity"]
            .as_i64()
            .ok_or(Error::InvalidRecord)?;
        if seconds < 120 {
            return Err(Error::InvalidRecord);
        }
        let until = chrono::Utc::now() + chrono::Duration::seconds(seconds.min(180 * 86400) - 60);
        let data = self.request(Method::POST, "/auth", &[], Some(json!({
            "access":{"valid_until":until.to_rfc3339(), "balances":true, "transactions":true},
            "aspsp":{"name":bank["name"], "country":"PL"}, "state":state,
            "redirect_url":crate::CALLBACK_URL, "psu_type":"personal", "language":"en",
        }))).await?;
        let url = data["url"].as_str().ok_or(Error::InvalidRecord)?;
        validate_auth_url(url)?;
        Ok(url.to_owned())
    }

    pub async fn authorize(&self, code: &str) -> Result<Value> {
        self.request(Method::POST, "/sessions", &[], Some(json!({"code":code})))
            .await
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        uuid::Uuid::parse_str(id).map_err(|_| Error::InvalidRecord)?;
        match self
            .request(Method::DELETE, &format!("/sessions/{id}"), &[], None)
            .await
        {
            Ok(_) | Err(Error::Expired) | Err(Error::Provider(404)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub async fn balances(&self, uid: &str) -> Result<Value> {
        uuid::Uuid::parse_str(uid).map_err(|_| Error::InvalidRecord)?;
        self.request(Method::GET, &format!("/accounts/{uid}/balances"), &[], None)
            .await
    }

    pub async fn transactions(&self, uid: &str, query: &[(&str, String)]) -> Result<Value> {
        uuid::Uuid::parse_str(uid).map_err(|_| Error::InvalidRecord)?;
        self.request(
            Method::GET,
            &format!("/accounts/{uid}/transactions"),
            query,
            None,
        )
        .await
    }
}

pub(crate) fn validate_auth_url(text: &str) -> Result<()> {
    let url = Url::parse(text).map_err(|_| Error::InvalidAuthUrl)?;
    // Enable Banking is migrating authorization from tilisy to auth. Accept
    // only these exact provider hosts, never arbitrary subdomains.
    // https://enablebanking.com/blog/2026/06/12/enable-banking-changelog-may-2026
    let provider_host = matches!(
        url.host_str(),
        Some("auth.enablebanking.com" | "tilisy.enablebanking.com")
    );
    if url.scheme() != "https"
        || !provider_host
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::InvalidAuthUrl);
    }
    Ok(())
}
