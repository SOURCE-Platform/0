-- Spec v0.5 §22.8 / wire annex A.2.1: peer-sync tokens for SOURCE Vault
-- phones, kept apart from mobile_devices (SOURCE Mobile tokens never
-- reach the vault peer route, and these never reach any other route).
-- Only SHA-256(token) is stored, bound to the vault registry device id.
CREATE TABLE IF NOT EXISTS vault_peer_tokens (
    device_id TEXT PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);
