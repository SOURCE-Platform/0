use crate::{types::Money, Error, Result};
use serde_json::Value;

// Keep the bank's decimal text. Never convert money to f32/f64.
pub fn parse_money(value: &Value) -> Result<Money> {
    let amount = value["amount"].as_str().ok_or(Error::InvalidRecord)?;
    let currency = value["currency"].as_str().ok_or(Error::InvalidRecord)?;
    let unsigned = amount.strip_prefix('-').unwrap_or(amount);
    let mut parts = unsigned.split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.is_some_and(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
        || parts.next().is_some()
        || amount.len() > 50
        || currency.len() != 3
        || !currency.bytes().all(|b| b.is_ascii_uppercase())
    {
        return Err(Error::InvalidRecord);
    }
    Ok(Money {
        amount: amount.to_owned(),
        currency: currency.to_owned(),
    })
}

pub fn date(value: &Value, key: &str) -> Result<Option<String>> {
    match value.get(key).filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(v) => {
            let text = v.as_str().ok_or(Error::InvalidRecord)?;
            chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .map_err(|_| Error::InvalidRecord)?;
            if text.len() != 10 {
                return Err(Error::InvalidRecord);
            }
            Ok(Some(text.to_owned()))
        }
    }
}
