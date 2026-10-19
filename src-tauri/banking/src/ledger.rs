use crate::{
    money::{date, parse_money},
    types::*,
    Error, Result,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn digest(value: &Value) -> String {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted: BTreeMap<_, _> = map.iter().collect();
                Value::Object(
                    sorted
                        .into_iter()
                        .map(|(k, v)| (k.clone(), canonical(v)))
                        .collect(),
                )
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }
    URL_SAFE_NO_PAD.encode(Sha256::digest(
        serde_json::to_vec(&canonical(value)).expect("JSON value"),
    ))
}

pub fn observe(raw: Value, now: &str) -> Observation {
    Observation {
        digest: digest(&raw),
        raw,
        observed_at: now.to_owned(),
    }
}

pub fn apply_session(ledger: &mut Ledger, data: &Value, bank: &Value, now: &str) -> Result<()> {
    if data["aspsp"]["country"] != "PL"
        || data["aspsp"]["name"] != bank["name"]
        || data["psu_type"] != "personal"
    {
        return Err(Error::InvalidRecord);
    }
    let id = data["session_id"]
        .as_str()
        .ok_or(Error::InvalidRecord)?
        .to_owned();
    uuid::Uuid::parse_str(&id).map_err(|_| Error::InvalidRecord)?;
    let until = data["access"]["valid_until"]
        .as_str()
        .ok_or(Error::InvalidRecord)?;
    let expiry = chrono::DateTime::parse_from_rfc3339(until).map_err(|_| Error::InvalidRecord)?;
    if expiry <= chrono::Utc::now() {
        return Err(Error::Expired);
    }
    let accounts = data["accounts"].as_array().ok_or(Error::InvalidRecord)?;
    if accounts.is_empty() {
        return Err(Error::EmptyAccounts);
    }
    for account in ledger.accounts.values_mut() {
        account.active = false;
    }
    let mut ids = Vec::new();
    for raw in accounts {
        let Some(uid) = raw["uid"].as_str() else {
            continue;
        };
        uuid::Uuid::parse_str(uid).map_err(|_| Error::InvalidRecord)?;
        let hash = raw["identification_hash"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(Error::InvalidRecord)?;
        let id = digest(&serde_json::json!(["enable-banking", "PL", hash]));
        if ids.contains(&id) {
            return Err(Error::InvalidRecord);
        }
        let currency = raw["currency"].as_str().ok_or(Error::InvalidRecord)?;
        parse_money(&serde_json::json!({"amount":"0", "currency":currency}))?;
        let previous = ledger.accounts.get(&id);
        let number = raw["account_id"]["iban"]
            .as_str()
            .or(raw["account_id"]["bban"].as_str());
        let masked_number = number.map(|n| {
            format!(
                "•••• {}",
                n.chars()
                    .rev()
                    .take(4)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            )
        });
        let account = Account {
            id: id.clone(),
            label: raw["product"]
                .as_str()
                .or(raw["details"].as_str())
                .unwrap_or("Millennium account")
                .to_owned(),
            currency: currency.to_owned(),
            masked_number,
            active: true,
            balances: previous.map(|a| a.balances.clone()).unwrap_or_default(),
            earliest_booked_date: previous.and_then(|a| a.earliest_booked_date.clone()),
            provider_uid: uid.to_owned(),
            raw: raw.clone(),
        };
        ledger.accounts.insert(id.clone(), account);
        ids.push(id);
    }
    if ids.is_empty() {
        return Err(Error::EmptyAccounts);
    }
    ledger.sessions.push(Session {
        id,
        valid_until: until.to_owned(),
        account_ids: ids,
    });
    ledger.session_observations.push(observe(data.clone(), now));
    Ok(())
}

pub fn apply_balances(ledger: &mut Ledger, id: &str, data: Value, now: &str) -> Result<()> {
    let rows = data["balances"].as_array().ok_or(Error::InvalidRecord)?;
    let balances = rows
        .iter()
        .map(|raw| {
            Ok(Balance {
                money: parse_money(&raw["balance_amount"])?,
                balance_type: raw["balance_type"]
                    .as_str()
                    .ok_or(Error::InvalidRecord)?
                    .to_owned(),
                observed_at: now.to_owned(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ledger
        .accounts
        .get_mut(id)
        .ok_or(Error::InvalidRecord)?
        .balances = balances;
    let history = ledger
        .balance_observations
        .entry(id.to_owned())
        .or_default();
    let observation = observe(data, now);
    if history.last().map(|o| &o.digest) != Some(&observation.digest) {
        history.push(observation);
    }
    Ok(())
}

pub fn apply_transactions(
    ledger: &mut Ledger,
    id: &str,
    rows: Vec<Value>,
    now: &str,
) -> Result<()> {
    let mut occurrences = BTreeMap::<String, usize>::new();
    let mut references = BTreeMap::<String, String>::new();
    let prior_pending: BTreeMap<_, _> = ledger
        .pending
        .iter()
        .filter(|t| t.account_id == id)
        .map(|t| (t.id.clone(), t.clone()))
        .collect();
    ledger.pending.retain(|t| t.account_id != id);
    for raw in rows {
        let status = raw["status"]
            .as_str()
            .ok_or(Error::InvalidRecord)?
            .to_owned();
        let booked = status == "BOOK";
        let reference = raw["entry_reference"].as_str().filter(|s| !s.is_empty());
        let mut stable_raw = raw.clone();
        stable_raw
            .as_object_mut()
            .ok_or(Error::InvalidRecord)?
            .remove("transaction_id");
        let content_hash = digest(&stable_raw);
        let key = if let Some(reference) = reference {
            // Duplicate entry references in one response are not silently merged.
            if references
                .insert(reference.to_owned(), content_hash.clone())
                .is_some()
            {
                return Err(Error::InvalidRecord);
            }
            digest(&serde_json::json!([id, "entry", reference]))
        } else {
            let count = occurrences.entry(content_hash.clone()).or_default();
            *count += 1;
            digest(&serde_json::json!([
                id,
                "observation",
                content_hash,
                *count
            ]))
        };
        let indicator = raw["credit_debit_indicator"]
            .as_str()
            .ok_or(Error::InvalidRecord)?;
        let direction = match indicator {
            "CRDT" => "incoming",
            "DBIT" => "outgoing",
            _ => return Err(Error::InvalidRecord),
        };
        let party = if direction == "incoming" {
            &raw["debtor"]
        } else {
            &raw["creditor"]
        };
        let description = raw["remittance_information"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Bank transaction".into());
        let was_committed = ledger.transactions.contains_key(&key);
        let previous = ledger
            .transactions
            .get(&key)
            .or_else(|| prior_pending.get(&key));
        let mut revisions = previous.map(|t| t.revisions.clone()).unwrap_or_default();
        // Retain the original transaction_id as evidence, but ignore its churn for identity.
        if revisions.last().map(|o| &o.digest) != Some(&content_hash) {
            let mut observation = observe(raw.clone(), now);
            observation.digest = content_hash;
            revisions.push(observation);
        }
        let transaction = Transaction {
            id: key.clone(),
            account_id: id.to_owned(),
            money: parse_money(&raw["transaction_amount"])?,
            direction: direction.into(),
            status,
            booking_date: date(&raw, "booking_date")?,
            value_date: date(&raw, "value_date")?,
            transaction_date: date(&raw, "transaction_date")?,
            description,
            counterparty: party["name"].as_str().map(str::to_owned),
            identity_quality: if reference.is_some() {
                "bank-reference"
            } else {
                "unverified-fingerprint"
            }
            .into(),
            first_seen_at: previous
                .map(|t| t.first_seen_at.clone())
                .unwrap_or_else(|| now.to_owned()),
            revisions,
        };
        if booked || was_committed {
            if let Some(date) = &transaction.booking_date {
                let account = ledger.accounts.get_mut(id).ok_or(Error::InvalidRecord)?;
                if account
                    .earliest_booked_date
                    .as_ref()
                    .is_none_or(|earliest| date < earliest)
                {
                    account.earliest_booked_date = Some(date.clone());
                }
            }
            ledger.transactions.insert(key, transaction);
        } else {
            ledger.pending.push(transaction);
        }
    }
    Ok(())
}
