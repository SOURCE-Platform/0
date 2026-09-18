//! §15 error codes (Phase C subset) as a closed enum. Error frames carry
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
            ErrorCode::ManifestMismatch => "MANIFEST_MISMATCH",
            ErrorCode::ManifestRollback => "MANIFEST_ROLLBACK",
            ErrorCode::IntegrityFailure => "INTEGRITY_FAILURE",
            ErrorCode::CaptureUnsafe => "CAPTURE_UNSAFE",
            ErrorCode::ConflictPending => "CONFLICT_PENDING",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::PanelCancelled => "PANEL_CANCELLED",
            ErrorCode::PresenceDenied => "PRESENCE_DENIED",
            ErrorCode::InvalidInput => "INVALID_INPUT",
            ErrorCode::Internal => "INTERNAL",
        }
    }

    pub fn frame(self) -> Value {
        json!({ "ok": false, "error": self.as_str() })
    }
}
