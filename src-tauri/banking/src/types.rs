use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Money {
    pub amount: String,
    pub currency: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    pub money: Money,
    pub balance_type: String,
    pub observed_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub label: String,
    pub currency: String,
    pub masked_number: Option<String>,
    pub active: bool,
    pub balances: Vec<Balance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earliest_booked_date: Option<String>,
    // Provider identifiers and originals never leave the Rust service.
    #[serde(default)]
    pub(crate) provider_uid: String,
    pub(crate) raw: Value,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub id: String,
    pub account_id: String,
    pub money: Money,
    pub direction: String,
    pub status: String,
    pub booking_date: Option<String>,
    pub value_date: Option<String>,
    pub transaction_date: Option<String>,
    pub description: String,
    pub counterparty: Option<String>,
    pub identity_quality: String,
    pub first_seen_at: String,
    pub revisions: Vec<Observation>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub observed_at: String,
    pub digest: String,
    pub raw: Value,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub valid_until: String,
    pub account_ids: Vec<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub session_observations: Vec<Observation>,
    pub accounts: BTreeMap<String, Account>,
    pub transactions: BTreeMap<String, Transaction>,
    pub pending: Vec<Transaction>,
    pub balance_observations: BTreeMap<String, Vec<Observation>>,
    #[serde(default)]
    pub transaction_batches: BTreeMap<String, Vec<Observation>>,
    pub auto_sync: bool,
    pub last_attempt_at: Option<String>,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub label: String,
    pub currency: String,
    pub masked_number: Option<String>,
    pub active: bool,
    pub balances: Vec<Balance>,
    pub earliest_booked_date: Option<String>,
}

impl From<&Account> for AccountView {
    fn from(a: &Account) -> Self {
        Self {
            id: a.id.clone(),
            label: a.label.clone(),
            currency: a.currency.clone(),
            masked_number: a.masked_number.clone(),
            active: a.active,
            balances: a.balances.clone(),
            earliest_booked_date: a.earliest_booked_date.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionView {
    pub id: String,
    pub account_id: String,
    pub money: Money,
    pub direction: String,
    pub status: String,
    pub date: Option<String>,
    pub description: String,
    pub counterparty: Option<String>,
    pub identity_quality: String,
    pub revision_count: usize,
}

impl From<&Transaction> for TransactionView {
    fn from(t: &Transaction) -> Self {
        Self {
            id: t.id.clone(),
            account_id: t.account_id.clone(),
            money: t.money.clone(),
            direction: t.direction.clone(),
            status: t.status.clone(),
            date: t
                .booking_date
                .clone()
                .or(t.transaction_date.clone())
                .or(t.value_date.clone()),
            description: t.description.clone(),
            counterparty: t.counterparty.clone(),
            identity_quality: t.identity_quality.clone(),
            revision_count: t.revisions.len(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransactionQuery {
    pub account_id: Option<String>,
    pub direction: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub search: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPage {
    pub rows: Vec<TransactionView>,
    pub total: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankingOverview {
    pub supported: bool,
    pub configured: bool,
    pub connected: bool,
    pub consent_valid_until: Option<String>,
    pub accounts: Vec<AccountView>,
    pub transaction_count: usize,
    pub pending_count: usize,
    pub auto_sync: bool,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub callback_url: &'static str,
}
