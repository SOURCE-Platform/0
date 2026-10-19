//! Personal, read-only banking. No payment endpoints or AI transport.
mod callback;
mod callback_page;
mod client;
mod error;
mod ledger;
mod money;
mod queries;
mod secrets;
mod service;
mod storage;
mod sync;
mod types;

pub use error::{Error, Result};
pub use service::BankingService;
pub use types::{BankingOverview, TransactionPage, TransactionQuery};

/// Registered with Enable Banking; its HTTPS route returns the browser to this Mac.
pub const CALLBACK_URL: &str = "https://sourceovcourse.com/banking/callback";
pub const LOCAL_CALLBACK_URL: &str = "http://127.0.0.1:1431/banking/callback";

#[cfg(test)]
mod client_tests;
#[cfg(test)]
mod storage_tests;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tests;
