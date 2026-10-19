use crate::{client::EnableBanking, ledger, types::Ledger, Error, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

#[allow(async_fn_in_trait)]
pub trait BankFeed {
    async fn balances(&self, uid: &str) -> Result<Value>;
    async fn transactions(&self, uid: &str, query: &[(&str, String)]) -> Result<Value>;
}

impl BankFeed for EnableBanking {
    async fn balances(&self, uid: &str) -> Result<Value> {
        self.balances(uid).await
    }
    async fn transactions(&self, uid: &str, query: &[(&str, String)]) -> Result<Value> {
        self.transactions(uid, query).await
    }
}

pub async fn import(ledger: &mut Ledger, api: &impl BankFeed, full: bool, now: &str) -> Result<()> {
    let accounts: Vec<_> = ledger
        .accounts
        .values()
        .filter(|a| a.active)
        .map(|a| {
            (
                a.id.clone(),
                a.provider_uid.clone(),
                a.earliest_booked_date.is_none(),
            )
        })
        .collect();
    if accounts.is_empty() {
        return Err(Error::NotConnected);
    }
    for (id, uid, new_account) in accounts {
        let balances = api.balances(&uid).await?;
        ledger::apply_balances(ledger, &id, balances, now)?;
        let all = fetch_transactions(api, &uid, full || new_account, now).await?;
        let observation = ledger::observe(json!(all), now);
        let history = ledger.transaction_batches.entry(id.clone()).or_default();
        if history.last().map(|o| &o.digest) != Some(&observation.digest) {
            history.push(observation);
        }
        ledger::apply_transactions(ledger, &id, all, now)?;
    }
    ledger.last_synced_at = Some(now.to_owned());
    ledger.last_error = None;
    Ok(())
}

async fn fetch_transactions(
    api: &impl BankFeed,
    uid: &str,
    full: bool,
    now: &str,
) -> Result<Vec<Value>> {
    let now = chrono::DateTime::parse_from_rfc3339(now).map_err(|_| Error::InvalidRecord)?;
    let base = if full {
        vec![("strategy", "longest".to_owned())]
    } else {
        // Re-read a conservative overlap, not just "after the last transaction".
        vec![
            (
                "date_from",
                (now - chrono::Duration::days(90)).date_naive().to_string(),
            ),
            ("date_to", now.date_naive().to_string()),
        ]
    };
    let mut query = base.clone();
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for _ in 0..1000 {
        let page = api.transactions(uid, &query).await?;
        let transactions = page["transactions"]
            .as_array()
            .ok_or(Error::InvalidRecord)?;
        if rows.len() + transactions.len() > 200_000 {
            return Err(Error::InvalidRecord);
        }
        rows.extend(transactions.iter().cloned());
        match page.get("continuation_key").filter(|v| !v.is_null()) {
            None => return Ok(rows),
            Some(v) if v.as_str() == Some("") => return Ok(rows),
            Some(v) => {
                let key = v
                    .as_str()
                    .filter(|k| k.len() <= 8192)
                    .ok_or(Error::InvalidRecord)?;
                if !seen.insert(key.to_owned()) {
                    return Err(Error::InvalidRecord);
                }
                query = base.clone();
                query.push(("continuation_key", key.to_owned()));
            }
        }
    }
    Err(Error::InvalidRecord)
}
