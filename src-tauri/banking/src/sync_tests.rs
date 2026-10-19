use crate::{
    sync::{self, BankFeed},
    tests::{ledger, transaction},
    Error, Result,
};
use serde_json::{json, Value};
use std::sync::Mutex;

struct Feed {
    calls: Mutex<Vec<Vec<(String, String)>>>,
    repeat_key: bool,
    fail: bool,
}

impl BankFeed for Feed {
    async fn balances(&self, _: &str) -> Result<Value> {
        Ok(
            json!({"balances":[{"balance_amount":{"amount":"1200.01","currency":"PLN"},"balance_type":"CLBD"}]}),
        )
    }
    async fn transactions(&self, _: &str, query: &[(&str, String)]) -> Result<Value> {
        self.calls.lock().unwrap().push(
            query
                .iter()
                .map(|(a, b)| (a.to_string(), b.clone()))
                .collect(),
        );
        if self.fail {
            return Err(Error::Network);
        }
        if !query.iter().any(|(k, _)| *k == "continuation_key") || self.repeat_key {
            Ok(
                json!({"transactions":[transaction(Some("first"),"BOOK","1.01")], "continuation_key":"opaque?key=&"}),
            )
        } else {
            Ok(json!({"transactions":[transaction(Some("second"),"BOOK","2.02")]}))
        }
    }
}

#[tokio::test]
async fn fetches_every_page_and_preserves_query_and_observations() {
    let feed = Feed {
        calls: Mutex::new(vec![]),
        repeat_key: false,
        fail: false,
    };
    let mut data = ledger();
    sync::import(&mut data, &feed, true, "2026-10-18T12:00:00Z")
        .await
        .unwrap();
    assert_eq!(data.transactions.len(), 2);
    let calls = feed.calls.lock().unwrap();
    assert!(calls[1].contains(&("continuation_key".into(), "opaque?key=&".into())));
    assert!(calls[1].contains(&("strategy".into(), "longest".into())));
    assert_eq!(
        data.transaction_batches.values().next().unwrap()[0]
            .raw
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn repeated_continuation_keys_fail_instead_of_looping_or_committing_partial_data() {
    let feed = Feed {
        calls: Mutex::new(vec![]),
        repeat_key: true,
        fail: false,
    };
    let original = ledger();
    let mut staged = original.clone();
    assert!(
        sync::import(&mut staged, &feed, true, "2026-10-18T12:00:00Z")
            .await
            .is_err()
    );
    assert!(original.transactions.is_empty());
    assert_eq!(feed.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn incremental_sync_uses_ninety_day_overlap() {
    let feed = Feed {
        calls: Mutex::new(vec![]),
        repeat_key: false,
        fail: false,
    };
    let mut data = ledger();
    data.accounts
        .values_mut()
        .next()
        .unwrap()
        .earliest_booked_date = Some("2025-01-01".into());
    sync::import(&mut data, &feed, false, "2026-10-18T12:00:00Z")
        .await
        .unwrap();
    let calls = feed.calls.lock().unwrap();
    assert!(calls[0].contains(&("date_from".into(), "2026-07-20".into())));
    assert!(calls[0].contains(&("date_to".into(), "2026-10-18".into())));
}
