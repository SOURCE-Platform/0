use crate::{
    callback::CallbackListener,
    client::{self, EnableBanking},
    ledger, queries,
    secrets::{Credentials, SecretStore, SystemSecrets},
    storage, sync,
    types::*,
    Error, Result,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{watch, Mutex};
use zeroize::Zeroizing;

pub struct BankingService {
    directory: PathBuf,
    secrets: Arc<dyn SecretStore>,
    operation: Mutex<()>,
    cancel: watch::Sender<u64>,
}

impl BankingService {
    pub fn new(directory: PathBuf) -> Self {
        Self::with_secrets(directory, Arc::new(SystemSecrets))
    }

    pub(crate) fn with_secrets(directory: PathBuf, secrets: Arc<dyn SecretStore>) -> Self {
        let (cancel, _) = watch::channel(0);
        Self {
            directory,
            secrets,
            operation: Mutex::new(()),
            cancel,
        }
    }

    fn credentials(&self) -> Result<Credentials> {
        let bytes = self
            .secrets
            .get("enable-banking")?
            .ok_or(Error::NotConfigured)?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Keychain)
    }

    fn load(&self) -> Result<(Zeroizing<Vec<u8>>, Ledger)> {
        let key = storage::key(self.secrets.as_ref(), &self.directory, false)?;
        let ledger = storage::load(&self.directory, &key)?;
        Ok((key, ledger))
    }

    fn view(&self, ledger: &Ledger, configured: bool) -> BankingOverview {
        BankingOverview {
            supported: cfg!(target_os = "macos"),
            configured,
            connected: !ledger.sessions.is_empty(),
            consent_valid_until: ledger.sessions.last().map(|s| s.valid_until.clone()),
            accounts: ledger.accounts.values().map(AccountView::from).collect(),
            transaction_count: ledger.transactions.len(),
            pending_count: ledger.pending.len(),
            auto_sync: ledger.auto_sync,
            last_synced_at: ledger.last_synced_at.clone(),
            last_error: ledger.last_error.clone(),
            callback_url: crate::CALLBACK_URL,
        }
    }

    pub async fn overview(&self) -> Result<BankingOverview> {
        let _guard = self.operation.try_lock().map_err(|_| Error::Busy)?;
        if !cfg!(target_os = "macos") {
            return Ok(self.view(&Ledger::default(), false));
        }
        let configured = self.secrets.get("enable-banking")?.is_some();
        match self.load() {
            Ok((_, ledger)) => Ok(self.view(&ledger, configured)),
            Err(Error::NotConfigured) if !configured => Ok(self.view(&Ledger::default(), false)),
            Err(e) => Err(e),
        }
    }

    pub async fn configure(
        &self,
        application_id: String,
        key_path: &Path,
    ) -> Result<BankingOverview> {
        let _guard = self.operation.try_lock().map_err(|_| Error::Busy)?;
        uuid::Uuid::parse_str(&application_id).map_err(|_| Error::InvalidApplication)?;
        let metadata = std::fs::symlink_metadata(key_path).map_err(|_| Error::InvalidKey)?;
        if !metadata.is_file() || metadata.len() > 65536 {
            return Err(Error::InvalidKey);
        }
        let pem = Zeroizing::new(std::fs::read_to_string(key_path).map_err(|_| Error::InvalidKey)?);
        client::signing_key(&pem)?;
        let key = storage::key(self.secrets.as_ref(), &self.directory, true)?;
        let ledger = storage::load(&self.directory, &key)?;
        if !ledger.sessions.is_empty() {
            return Err(Error::Connected);
        }
        let credentials = Credentials {
            application_id,
            private_key: pem.to_string(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&credentials).map_err(|_| Error::Keychain)?);
        storage::save(&self.directory, &key, &ledger)?;
        self.secrets.put("enable-banking", &bytes)?;
        Ok(self.view(&ledger, true))
    }

    pub fn cancel_connect(&self) {
        self.cancel.send_modify(|n| *n = n.wrapping_add(1));
    }

    pub async fn connect(
        &self,
        open_browser: impl FnOnce(String) -> Result<()>,
    ) -> Result<BankingOverview> {
        let _guard = self.operation.try_lock().map_err(|_| Error::Busy)?;
        let cancel = self.cancel.subscribe();
        let api = EnableBanking::new(&self.credentials()?)?;
        let (key, mut ledger) = self.load()?;
        let bank = api.millennium().await?;
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|_| Error::Network)?;
        let nonce = URL_SAFE_NO_PAD.encode(nonce);
        let mut callback = CallbackListener::bind(nonce.clone()).await?;
        let url = api.start_auth(&bank, &nonce).await?;
        if cancel.has_changed().unwrap_or(true) {
            return Err(Error::Cancelled);
        }
        open_browser(url)?;
        let code = Zeroizing::new(callback.wait(cancel).await?);
        drop(callback);
        let response = api.authorize(&code).await?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut next = ledger.clone();
        if let Err(error) = ledger::apply_session(&mut next, &response, &bank, &now) {
            // Keep the new session available for revocation if the bank returned no
            // usable accounts and the best-effort remote cleanup also fails.
            if let Some(id) = response["session_id"].as_str() {
                if api.delete_session(id).await.is_err() {
                    uuid::Uuid::parse_str(id).map_err(|_| Error::InvalidRecord)?;
                    ledger.sessions.push(Session {
                        id: id.into(),
                        valid_until: now.clone(),
                        account_ids: vec![],
                    });
                    ledger.last_error = Some(error.to_string());
                    storage::save(&self.directory, &key, &ledger)?;
                }
            }
            return Err(error);
        }
        // Persist the consent before fetching history, so failed imports remain retryable.
        if let Err(error) = storage::save(&self.directory, &key, &next) {
            let id = &next.sessions.last().ok_or(Error::InvalidRecord)?.id;
            if api.delete_session(id).await.is_err() {
                return Err(Error::SessionNotSaved);
            }
            return Err(error);
        }
        self.run_sync(&api, &key, next, true).await
    }

    async fn run_sync(
        &self,
        api: &EnableBanking,
        key: &[u8],
        mut ledger: Ledger,
        full: bool,
    ) -> Result<BankingOverview> {
        let now = chrono::Utc::now().to_rfc3339();
        ledger.last_attempt_at = Some(now.clone());
        storage::save(&self.directory, key, &ledger)?;
        let mut staged = ledger.clone();
        match sync::import(&mut staged, api, full, &now).await {
            Ok(()) => {
                storage::save(&self.directory, key, &staged)?;
                Ok(self.view(&staged, true))
            }
            Err(error) => {
                ledger.last_error = Some(error.to_string());
                storage::save(&self.directory, key, &ledger)?;
                Err(error)
            }
        }
    }

    pub async fn sync(&self, background: bool) -> Result<BankingOverview> {
        let _guard = self.operation.try_lock().map_err(|_| Error::Busy)?;
        let (key, ledger) = self.load()?;
        if background && !ledger.auto_sync {
            return Ok(self.view(&ledger, true));
        }
        if ledger.sessions.is_empty() {
            return Err(Error::NotConnected);
        }
        let elapsed = ledger
            .last_attempt_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| chrono::Utc::now().signed_duration_since(t).num_seconds());
        let minimum = if background { 6 * 3600 } else { 60 };
        if elapsed.is_some_and(|s| s < minimum) {
            return if background {
                Ok(self.view(&ledger, true))
            } else {
                Err(Error::SyncTooSoon)
            };
        }
        let api = EnableBanking::new(&self.credentials()?)?;
        self.run_sync(&api, &key, ledger, false).await
    }

    pub async fn background_tick(&self) {
        // An unconfigured installation never prompts for Keychain on a timer.
        if !self.directory.join("ledger.enc").exists() {
            return;
        }
        let _ = self.sync(true).await;
    }

    pub async fn set_auto_sync(&self, enabled: bool) -> Result<BankingOverview> {
        let _guard = self.operation.try_lock().map_err(|_| Error::Busy)?;
        let (key, mut ledger) = self.load()?;
        ledger.auto_sync = enabled;
        storage::save(&self.directory, &key, &ledger)?;
        Ok(self.view(&ledger, true))
    }

    pub async fn disconnect(&self) -> Result<BankingOverview> {
        let _guard = self.operation.try_lock().map_err(|_| Error::Busy)?;
        let (key, mut ledger) = self.load()?;
        let api = EnableBanking::new(&self.credentials()?)?;
        // Keep all session IDs until every revocation succeeds, making retries safe.
        for session in &ledger.sessions {
            api.delete_session(&session.id).await?;
        }
        ledger.sessions.clear();
        ledger.auto_sync = false;
        ledger.pending.clear();
        for account in ledger.accounts.values_mut() {
            account.active = false;
        }
        ledger.last_error = None;
        storage::save(&self.directory, &key, &ledger)?;
        Ok(self.view(&ledger, true))
    }

    pub async fn transactions(&self, query: TransactionQuery) -> Result<TransactionPage> {
        let _guard = self.operation.try_lock().map_err(|_| Error::Busy)?;
        let (_, ledger) = self.load()?;
        queries::transactions(&ledger, query)
    }
}
