use crate::{ledger, money, queries, storage, types::*, Error};
use serde_json::{json, Value};

const NOW: &str = "2026-10-18T12:00:00Z";

pub(crate) fn account() -> Value {
    json!({
        "uid":"00000000-0000-4000-8000-000000000001", "identification_hash":"synthetic-hash",
        "currency":"PLN", "product":"Personal", "account_id":{"iban":"PL00000000000000000000000001"}
    })
}

pub(crate) fn session(accounts: Vec<Value>) -> Value {
    json!({
        "session_id":"00000000-0000-4000-8000-000000000002", "accounts":accounts,
        "aspsp":{"name":"Bank Millennium", "country":"PL"}, "psu_type":"personal",
        "access":{"valid_until":"2099-01-01T00:00:00Z"}
    })
}

pub(crate) fn ledger() -> Ledger {
    let mut ledger = Ledger::default();
    ledger::apply_session(
        &mut ledger,
        &session(vec![account()]),
        &json!({"name":"Bank Millennium"}),
        NOW,
    )
    .unwrap();
    ledger
}

pub(crate) fn transaction(reference: Option<&str>, status: &str, amount: &str) -> Value {
    let mut value = json!({
        "transaction_amount":{"amount":amount,"currency":"PLN"}, "status":status,
        "credit_debit_indicator":"DBIT", "booking_date":"2026-10-17",
        "remittance_information":["Synthetic grocery payment"], "creditor":{"name":"Example merchant"},
        "transaction_id":"unstable-detail-id"
    });
    if let Some(reference) = reference {
        value["entry_reference"] = json!(reference);
    }
    value
}

#[test]
fn money_remains_exact_and_rejects_unsafe_formats() {
    assert_eq!(
        money::parse_money(&json!({"amount":"9007199254740993.01","currency":"PLN"}))
            .unwrap()
            .amount,
        "9007199254740993.01"
    );
    for amount in ["1e3", "NaN", "1,20", "1.", ".01", "--1", "1.2.3", ""] {
        assert!(money::parse_money(&json!({"amount":amount,"currency":"PLN"})).is_err());
    }
    assert!(money::parse_money(&json!({"amount":1.23,"currency":"PLN"})).is_err());
}

#[test]
fn reauthorization_matches_stable_account_hash_not_session_uid() {
    let mut data = ledger();
    let original = data.accounts.keys().next().unwrap().clone();
    let mut next = account();
    next["uid"] = json!("00000000-0000-4000-8000-000000000003");
    ledger::apply_session(
        &mut data,
        &session(vec![next]),
        &json!({"name":"Bank Millennium"}),
        NOW,
    )
    .unwrap();
    assert_eq!(data.accounts.len(), 1);
    assert_eq!(
        data.accounts[&original].provider_uid,
        "00000000-0000-4000-8000-000000000003"
    );
    let view = serde_json::to_string(&AccountView::from(&data.accounts[&original])).unwrap();
    assert!(
        !view.contains("synthetic-hash")
            && !view.contains("providerUid")
            && !view.contains("PL000")
    );
}

#[test]
fn stable_transaction_reimports_are_idempotent_and_changes_are_revisions() {
    let mut data = ledger();
    let id = data.accounts.keys().next().unwrap().clone();
    let mut raw = transaction(Some("bank-ref"), "BOOK", "12.34");
    ledger::apply_transactions(&mut data, &id, vec![raw.clone()], NOW).unwrap();
    raw["transaction_id"] = json!("another-detail-id");
    ledger::apply_transactions(&mut data, &id, vec![raw.clone()], NOW).unwrap();
    assert_eq!(data.transactions.len(), 1);
    assert_eq!(
        data.transactions.values().next().unwrap().revisions.len(),
        1
    );
    raw["remittance_information"] = json!(["Corrected bank description"]);
    ledger::apply_transactions(&mut data, &id, vec![raw], NOW).unwrap();
    let tx = data.transactions.values().next().unwrap();
    assert_eq!(tx.revisions.len(), 2);
    assert_eq!(
        tx.revisions[0].raw["remittance_information"][0],
        "Synthetic grocery payment"
    );
}

#[test]
fn pending_is_not_double_counted_as_booked_and_identical_purchases_are_preserved() {
    let mut data = ledger();
    let id = data.accounts.keys().next().unwrap().clone();
    ledger::apply_transactions(
        &mut data,
        &id,
        vec![transaction(None, "PDNG", "12.34")],
        NOW,
    )
    .unwrap();
    assert_eq!(data.pending.len(), 1);
    assert!(data.transactions.is_empty());
    let raw = transaction(None, "BOOK", "12.34");
    ledger::apply_transactions(&mut data, &id, vec![raw.clone(), raw.clone()], NOW).unwrap();
    ledger::apply_transactions(&mut data, &id, vec![raw.clone(), raw], NOW).unwrap();
    assert!(data.pending.is_empty());
    assert_eq!(data.transactions.len(), 2);
    assert!(data
        .transactions
        .values()
        .all(|t| t.identity_quality == "unverified-fingerprint"));
}

#[test]
fn colliding_bank_references_do_not_silently_overwrite() {
    let mut data = ledger();
    let id = data.accounts.keys().next().unwrap().clone();
    assert!(ledger::apply_transactions(
        &mut data,
        &id,
        vec![
            transaction(Some("duplicate"), "BOOK", "1.00"),
            transaction(Some("duplicate"), "BOOK", "2.00"),
        ],
        NOW
    )
    .is_err());
}

#[test]
fn empty_accounts_and_wrong_bank_are_rejected() {
    assert!(matches!(
        ledger::apply_session(
            &mut Ledger::default(),
            &session(vec![]),
            &json!({"name":"Bank Millennium"}),
            NOW
        ),
        Err(Error::EmptyAccounts)
    ));
    assert!(ledger::apply_session(
        &mut Ledger::default(),
        &session(vec![account()]),
        &json!({"name":"Other bank"}),
        NOW
    )
    .is_err());
}

#[test]
fn query_filters_are_bounded_and_never_return_raw_records() {
    let mut data = ledger();
    let id = data.accounts.keys().next().unwrap().clone();
    ledger::apply_transactions(
        &mut data,
        &id,
        vec![transaction(Some("ref"), "BOOK", "12.34")],
        NOW,
    )
    .unwrap();
    let page = queries::transactions(
        &data,
        TransactionQuery {
            search: Some("EXAMPLE".into()),
            date_from: Some("2026-10-01".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.total, 1);
    let text = serde_json::to_string(&page).unwrap();
    assert!(!text.contains("unstable-detail-id") && !text.contains("revisions\""));
    assert!(queries::transactions(
        &data,
        TransactionQuery {
            date_from: Some("2026-11-01".into()),
            date_to: Some("2026-10-01".into()),
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn ledger_is_encrypted_authenticated_and_never_silently_reset() {
    let dir = tempfile::tempdir().unwrap();
    let data = ledger();
    storage::save(dir.path(), &[7; 32], &data).unwrap();
    let path = dir.path().join("ledger.enc");
    let mut bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-hash"));
    assert_eq!(
        storage::load(dir.path(), &[7; 32]).unwrap().accounts.len(),
        1
    );
    assert!(matches!(
        storage::load(dir.path(), &[8; 32]),
        Err(Error::Integrity)
    ));
    let end = bytes.len() - 1;
    bytes[end] ^= 1;
    std::fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        storage::load(dir.path(), &[7; 32]),
        Err(Error::Integrity)
    ));
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn authorization_url_is_provider_https_only() {
    for host in ["auth.enablebanking.com", "tilisy.enablebanking.com"] {
        for suffix in ["/ais/start?sessionid=synthetic", ":443/ais/"] {
            assert!(crate::client::validate_auth_url(&format!("https://{host}{suffix}")).is_ok());
        }
        for url in [
            format!("http://{host}/ais/"),
            format!("https://{host}.evil.invalid/ais/"),
            format!("https://user@{host}/ais/"),
            format!("https://user:secret@{host}/ais/"),
            format!("https://{host}:8443/ais/"),
            format!("https://{host}@evil.invalid/ais/"),
        ] {
            assert!(crate::client::validate_auth_url(&url).is_err(), "{url}");
        }
    }
    for url in [
        "not a URL",
        "https://evil.invalid",
        "https://enablebanking.com/ais/",
        "https://api.enablebanking.com/ais/",
        "https://sub.tilisy.enablebanking.com/ais/",
        "https://tilisy.enablebanking.com./ais/",
    ] {
        assert!(crate::client::validate_auth_url(url).is_err(), "{url}");
    }
}

#[test]
fn bank_referenced_pending_to_booked_and_cancellation_keep_one_revision_chain() {
    let mut data = ledger();
    let id = data.accounts.keys().next().unwrap().clone();
    ledger::apply_transactions(
        &mut data,
        &id,
        vec![transaction(Some("stable"), "PDNG", "12.34")],
        NOW,
    )
    .unwrap();
    ledger::apply_transactions(
        &mut data,
        &id,
        vec![transaction(Some("stable"), "BOOK", "12.34")],
        NOW,
    )
    .unwrap();
    assert_eq!(data.transactions.len(), 1);
    assert!(data.pending.is_empty());
    assert_eq!(
        data.transactions.values().next().unwrap().revisions.len(),
        2
    );
    ledger::apply_transactions(
        &mut data,
        &id,
        vec![transaction(Some("stable"), "CNCL", "12.34")],
        NOW,
    )
    .unwrap();
    assert_eq!(data.transactions.len(), 1);
    assert!(data.pending.is_empty());
    let tx = data.transactions.values().next().unwrap();
    assert_eq!(tx.status, "CNCL");
    assert_eq!(tx.revisions.len(), 3);
}
