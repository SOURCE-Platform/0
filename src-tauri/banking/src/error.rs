pub type Result<T> = std::result::Result<T, Error>;

// Never include HTTP response bodies, URLs, file contents, or OS error details.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Banking currently requires macOS Keychain.")]
    Unsupported,
    #[error(
        "Keychain is unavailable or access was denied. Banking could not complete this operation."
    )]
    Keychain,
    #[error("Configure an Enable Banking application and private key first.")]
    NotConfigured,
    #[error("The private key must be a valid RSA PEM key of at least 2048 bits.")]
    InvalidKey,
    #[error("The application ID must be a UUID.")]
    InvalidApplication,
    #[error("Banking storage could not be read or saved. Existing records were not reset.")]
    Storage,
    #[error("The bank authorized access, but SOURCE could not save or revoke that session. Revoke its consent in Enable Banking or the bank before retrying.")]
    SessionNotSaved,
    #[error("Banking records could not be authenticated. Restore the original data and Keychain key; do not reset them.")]
    Integrity,
    #[error("Another banking operation is running.")]
    Busy,
    #[error("No bank is connected.")]
    NotConnected,
    #[error("The bank connection expired. Reconnect to continue importing.")]
    Expired,
    #[error("The provider is rate-limiting requests. Wait before syncing again.")]
    RateLimited,
    #[error("The provider requires end-user network headers this desktop integration does not yet supply.")]
    UserHeaders,
    #[error("The banking provider could not be reached. Existing records were retained.")]
    Network,
    #[error("The banking provider rejected the request (HTTP {0}). Check application setup and consent.")]
    Provider(u16),
    #[error(
        "The bank returned an unsupported or incomplete record. The import was not committed."
    )]
    InvalidRecord,
    #[error("No accounts were returned. In restricted mode, whitelist every account in Enable Banking first.")]
    EmptyAccounts,
    #[error(
        "Bank Millennium personal account access is not currently available from this application."
    )]
    BankUnavailable,
    #[error("The bank authorization URL was not on the expected Enable Banking HTTPS host.")]
    InvalidAuthUrl,
    #[error("Authorization was cancelled or timed out. You can try connecting again.")]
    Cancelled,
    #[error("The bank authorization callback port (1431) is unavailable. Do not stop an unrelated service.")]
    CallbackPort,
    #[error("The system browser could not be opened.")]
    Browser,
    #[error("Disconnect the bank before replacing application credentials.")]
    Connected,
    #[error("Wait at least one minute between manual sync attempts.")]
    SyncTooSoon,
    #[error("The requested transaction date range is invalid.")]
    InvalidQuery,
}
