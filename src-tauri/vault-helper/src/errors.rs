//! §15 error codes (through spec v0.4 Phase F) as a closed enum. Error frames carry
//! only the code string — never secret material, never usernames/URLs
//! (§15 universal rules).

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // Protocol/state
    BadState,
    UnknownOp,
    ProtocolViolation,
    // Credentials and wraps
    WrongCredential,
    RecoveryKeyInvalid,
    WrapCorrupt,
    // Vault data
    DbCorrupt,
    RecordCorrupt,
    FormatTooNew,
    /// Malformed or retired format (v0.4: v1 objects/DBs are refused).
    FormatInvalid,
    ManifestMismatch,
    ManifestRollback,
    IntegrityFailure,
    // Releases
    CaptureUnsafe,
    ConflictPending,
    NotFound,
    // Panels and presence
    PanelCancelled,
    /// LA presence sheet denied/cancelled/unavailable. Internal code:
    /// the §15 catalog names no code for a refused presence prompt; this
    /// one is documented in the Phase C verification report.
    PresenceDenied,
    // Registry / recovery (Phase D)
    SignatureInvalid,
    DeviceNotAuthorized,
    RegistryFork,
    RegistryTruncated,
    RotationFailed,
    RecoveryIncomplete,
    FinalizeConflict,
    BackupObjectMissing,
    BackupConflict,
    BackupUnavailable,
    // Provider protocol / backup (Phase F, §15 v0.4)
    SigningRefused,
    BackupReplay,
    BackupRevocationFailed,
    BackupStale,
    BackupAccessLost,
    KeychainUnavailable,
    TransferInvalid,
    TransferAborted,
    HandleTaken,
    RecoveryAuthStale,
    StateMoved,
    KdfPolicyViolation,
    RecoveryMetadataMismatch,
    RecoveryThrottled,
    CounterRegression,
    RevokedAuthorRefused,
    // Generic
    InvalidInput,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::BadState => "BAD_STATE",
            ErrorCode::UnknownOp => "UNKNOWN_OP",
            ErrorCode::ProtocolViolation => "PROTOCOL_VIOLATION",
            ErrorCode::WrongCredential => "WRONG_CREDENTIAL",
            ErrorCode::RecoveryKeyInvalid => "RECOVERY_KEY_INVALID",
            ErrorCode::WrapCorrupt => "WRAP_CORRUPT",
            ErrorCode::DbCorrupt => "DB_CORRUPT",
            ErrorCode::RecordCorrupt => "RECORD_CORRUPT",
            ErrorCode::FormatTooNew => "FORMAT_TOO_NEW",
            ErrorCode::FormatInvalid => "FORMAT_INVALID",
            ErrorCode::ManifestMismatch => "MANIFEST_MISMATCH",
            ErrorCode::ManifestRollback => "MANIFEST_ROLLBACK",
            ErrorCode::IntegrityFailure => "INTEGRITY_FAILURE",
            ErrorCode::CaptureUnsafe => "CAPTURE_UNSAFE",
            ErrorCode::ConflictPending => "CONFLICT_PENDING",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::PanelCancelled => "PANEL_CANCELLED",
            ErrorCode::PresenceDenied => "PRESENCE_DENIED",
            ErrorCode::InvalidInput => "INVALID_INPUT",
            ErrorCode::SignatureInvalid => "SIGNATURE_INVALID",
            ErrorCode::DeviceNotAuthorized => "DEVICE_NOT_AUTHORIZED",
            ErrorCode::RegistryFork => "REGISTRY_FORK",
            ErrorCode::RegistryTruncated => "REGISTRY_TRUNCATED",
            ErrorCode::RotationFailed => "ROTATION_FAILED",
            ErrorCode::RecoveryIncomplete => "RECOVERY_INCOMPLETE",
            ErrorCode::FinalizeConflict => "FINALIZE_CONFLICT",
            ErrorCode::BackupObjectMissing => "BACKUP_OBJECT_MISSING",
            ErrorCode::BackupConflict => "BACKUP_CONFLICT",
            ErrorCode::BackupUnavailable => "BACKUP_UNAVAILABLE",
            ErrorCode::SigningRefused => "SIGNING_REFUSED",
            ErrorCode::BackupReplay => "BACKUP_REPLAY",
            ErrorCode::BackupRevocationFailed => "BACKUP_REVOCATION_FAILED",
            ErrorCode::BackupStale => "BACKUP_STALE",
            ErrorCode::BackupAccessLost => "BACKUP_ACCESS_LOST",
            ErrorCode::KeychainUnavailable => "KEYCHAIN_UNAVAILABLE",
            ErrorCode::TransferInvalid => "TRANSFER_INVALID",
            ErrorCode::TransferAborted => "TRANSFER_ABORTED",
            ErrorCode::HandleTaken => "HANDLE_TAKEN",
            ErrorCode::RecoveryAuthStale => "RECOVERY_AUTH_STALE",
            ErrorCode::StateMoved => "STATE_MOVED",
            ErrorCode::KdfPolicyViolation => "KDF_POLICY_VIOLATION",
            ErrorCode::RecoveryMetadataMismatch => "RECOVERY_METADATA_MISMATCH",
            ErrorCode::RecoveryThrottled => "RECOVERY_THROTTLED",
            ErrorCode::CounterRegression => "COUNTER_REGRESSION",
            ErrorCode::RevokedAuthorRefused => "REVOKED_AUTHOR_REFUSED",
            ErrorCode::Internal => "INTERNAL",
        }
    }

    pub fn frame(self) -> Value {
        json!({ "ok": false, "error": self.as_str() })
    }
}
