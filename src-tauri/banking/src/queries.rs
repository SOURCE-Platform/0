use crate::{types::*, Error, Result};

pub fn transactions(ledger: &Ledger, query: TransactionQuery) -> Result<TransactionPage> {
    for value in [&query.date_from, &query.date_to].into_iter().flatten() {
        if value.len() != 10 || chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err() {
            return Err(Error::InvalidQuery);
        }
    }
    if query
        .date_from
        .as_ref()
        .zip(query.date_to.as_ref())
        .is_some_and(|(a, b)| a > b)
        || query
            .direction
            .as_deref()
            .is_some_and(|s| s != "incoming" && s != "outgoing")
        || query.search.as_ref().is_some_and(|s| s.len() > 500)
    {
        return Err(Error::InvalidQuery);
    }
    let search = query.search.unwrap_or_default().to_lowercase();
    let mut rows: Vec<_> = ledger
        .transactions
        .values()
        .chain(ledger.pending.iter())
        .map(TransactionView::from)
        .filter(|t| {
            query
                .account_id
                .as_ref()
                .is_none_or(|id| id == &t.account_id)
                && query.direction.as_ref().is_none_or(|d| d == &t.direction)
                && query
                    .date_from
                    .as_ref()
                    .is_none_or(|d| t.date.as_ref().is_some_and(|date| date >= d))
                && query
                    .date_to
                    .as_ref()
                    .is_none_or(|d| t.date.as_ref().is_some_and(|date| date <= d))
                && (search.is_empty()
                    || t.description.to_lowercase().contains(&search)
                    || t.counterparty
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&search))
        })
        .collect();
    rows.sort_by(|a, b| b.date.cmp(&a.date).then(a.id.cmp(&b.id)));
    let total = rows.len();
    Ok(TransactionPage {
        total,
        rows: rows
            .into_iter()
            .skip(query.offset.unwrap_or(0))
            .take(query.limit.unwrap_or(100).clamp(1, 250))
            .collect(),
    })
}
