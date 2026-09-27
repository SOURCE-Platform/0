//! Typed frontend bindings for the vault Tauri commands (Phase C).
//!
//! Every call is one helper op; error strings are the helper's §15 codes.
//! No secret is cached in the frontend: reveal results live in component
//! state only, are never persisted, and clear on lock/unmount.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type VaultState =
  | "uninitialized"
  | "locked"
  | "unlocking"
  | "unlocked"
  | "authorizing"
  | "backing_up"
  | "syncing"
  | "recovering"
  | "error"
  | "compromised"
  | "helper-unavailable"
  | "unknown";

export interface VaultItemMeta {
  ref: string;
  kind: string;
  title: string;
  username: string | null;
  hosts: string[];
  conflicted?: boolean;
  corrupt?: boolean;
}

export interface VaultEvent {
  event: string;
  state?: string;
  reason?: string;
  visible?: boolean;
  title?: string | null;
  op?: string;
}

interface StateResponse {
  state: string;
}

interface ListResponse {
  items: VaultItemMeta[];
}

interface RevealResponse {
  secret: Record<string, string>;
}

export async function vaultState(): Promise<VaultState> {
  const resp = await invoke<StateResponse>("vault_state");
  return (resp.state as VaultState) ?? "unknown";
}

/**
 * Create the vault. `handle` is the public recovery name (it can be an
 * email address) used to find the backup after losing every device.
 */
export async function vaultSetup(handle: string): Promise<void> {
  await invoke("vault_setup", { handle });
}

/**
 * The first recovery name was taken (BK-28). The helper re-checks the
 * master password and shows a new Recovery Key; the earlier sheet is void.
 */
export async function vaultSetupRetryHandle(handle: string): Promise<void> {
  await invoke("vault_setup_retry_handle", { handle });
}

export interface BackupStatus {
  state: string;
  last_success: number | null;
  last_error: string | null;
  attempts: number;
  stale: boolean;
  /** §11.3.2 remote-completion status (helper `remote_update_status`). */
  pending: RemotePending | null;
  /** Components the last commit made effective at the backup. */
  cleared: string[];
}

export interface RemotePending {
  pending: boolean;
  ops?: string[];
  security_driven?: boolean;
  needs_user?: boolean;
  attempts?: number;
  revocation_failed?: boolean;
}

export async function vaultBackupStatus(): Promise<{ status: BackupStatus; provider: string | null }> {
  return invoke("vault_backup_status");
}

export async function vaultBackupNow(): Promise<void> {
  await invoke("vault_backup_now");
}

export function onBackupStatus(handler: (s: BackupStatus) => void): Promise<UnlistenFn> {
  return listen<BackupStatus>("vault:backup", (e) => handler(e.payload));
}

export interface RecoveryPreview {
  vault_id: string;
  generation: number;
  created_at: number;
  item_count: number;
  registry_head_prefix: string;
}

/** Find the backup by recovery name and check it (the helper's own panel asks for the secret). */
export async function vaultRecoveryStart(handle: string, kind: "mp" | "rk"): Promise<RecoveryPreview> {
  const resp = await invoke<{ preview: RecoveryPreview }>("vault_recovery_start", { handle, kind });
  return resp.preview;
}

export async function vaultRecoveryFinish(): Promise<void> {
  await invoke("vault_recovery_finish");
}

/**
 * Unlock. Uses this device's own envelope — a presence check and the
 * Secure Enclave — and falls back to the master password only when this
 * device has no usable envelope (§2.8).
 */
export async function vaultUnlock(): Promise<void> {
  await invoke("vault_unlock");
}

/** Unlock with the master password regardless of the device envelope. */
export async function vaultUnlockWithMasterPassword(): Promise<void> {
  await invoke("vault_unlock_with_master_password");
}

/** Helper panel collects current + new master password (UNLOCKED only). */
export async function vaultChangeMasterPassword(): Promise<void> {
  await invoke("vault_change_master_password");
}

/** Helper panel collects the 24 Recovery Key words (LOCKED only). */
export async function vaultUnlockWithRecoveryKey(): Promise<void> {
  await invoke("vault_unlock_with_recovery_key");
}

/** New Recovery Key + key rotation; the helper shows/prints the words. */
export async function vaultRotateRecoveryKey(): Promise<void> {
  await invoke("vault_rotate_recovery_key");
}

/** New master password for an unlocked vault (old one forgotten). */
export async function vaultResetMasterPassword(): Promise<void> {
  await invoke("vault_reset_master_password");
}

export async function vaultLock(): Promise<void> {
  await invoke("vault_lock");
}

export async function vaultListItems(): Promise<VaultItemMeta[]> {
  const resp = await invoke<ListResponse>("vault_list_items");
  return resp.items ?? [];
}

export async function vaultAddLogin(
  title: string,
  username: string,
  host: string,
  password: string,
): Promise<string> {
  const resp = await invoke<{ ref: string }>("vault_add_login", {
    title,
    username,
    host,
    password,
  });
  return resp.ref;
}

export async function vaultUpdateItem(
  reference: string,
  field: string,
  value: string,
): Promise<void> {
  await invoke("vault_update_item", { reference, field, value });
}

export async function vaultDeleteItem(reference: string): Promise<void> {
  await invoke("vault_delete_item", { reference });
}

export async function vaultReveal(
  reference: string,
): Promise<Record<string, string>> {
  const resp = await invoke<RevealResponse>("vault_reveal", { reference });
  return resp.secret ?? {};
}

export async function vaultSetAutoLockMinutes(minutes: number): Promise<void> {
  await invoke("vault_set_auto_lock_minutes", { minutes });
}

/** Subscribe to helper events (state, locked, secure_panel_visible, ...). */
export function onVaultEvent(
  callback: (event: VaultEvent) => void,
): Promise<UnlistenFn> {
  return listen<VaultEvent>("vault:event", (e) => callback(e.payload));
}

/** §15 user-facing copy for the codes this surface can produce. */
export function vaultErrorMessage(code: string): string {
  if (code.startsWith("HELPER_UNAVAILABLE")) {
    return "The vault helper isn't available. Build and sign it (scripts/build-helper.sh) and try again.";
  }
  switch (code) {
    case "WRONG_CREDENTIAL":
      return "Incorrect password.";
    case "PRESENCE_DENIED":
      return "Confirmation was not completed.";
    case "PANEL_CANCELLED":
      return "The secure panel was closed.";
    case "CAPTURE_UNSAFE":
      return "Screen-capture exclusion is unavailable, so Source won't display this right now.";
    case "BAD_STATE":
      return "The vault isn't in the right state for that.";
    case "MANIFEST_ROLLBACK":
      return "Vault data appears rolled back; unlock was refused.";
    case "MANIFEST_MISMATCH":
      return "Vault data failed its integrity check.";
    case "WRAP_CORRUPT":
      return "This unlock method is damaged — use another.";
    case "FORMAT_TOO_NEW":
      return "This data was written by a newer version of Source — update the app.";
    case "DB_CORRUPT":
      return "The vault database is damaged. Restore from a backup.";
    case "RECOVERY_KEY_INVALID":
      return "That isn't a valid Recovery Key. Check the 24 words and try again.";
    case "ROTATION_FAILED":
      return "The security update didn't finish. The vault is locked; unlock to retry.";
    case "INVALID_INPUT":
      return "That input isn't valid.";
    // v0.4 backup and recovery (§15 copy).
    case "HANDLE_TAKEN":
      return "That recovery name is taken — choose another.";
    case "KDF_POLICY_VIOLATION":
      return "The backup service returned unsupported settings — recovery stopped.";
    case "RECOVERY_METADATA_MISMATCH":
      return "The backup service returned inconsistent data — recovery stopped.";
    case "RECOVERY_THROTTLED":
      return "Too many recovery attempts — try again later, or recover with your Recovery Key.";
    case "AUTH_INVALID":
      return "No backup matched that recovery name and password or Recovery Key.";
    case "BACKUP_UNAVAILABLE":
      return "The backup service can't be reached right now. Your vault keeps working on this Mac.";
    case "BACKUP_CONFLICT":
      return "Two devices changed the vault — review.";
    case "BACKUP_ACCESS_LOST":
      return "Backup access lost — this Mac may have been removed, or your vault may have been recovered on another device. If you did not do this, treat it as a security incident.";
    case "CONFLICT_PENDING":
      return "This item was changed on two devices — choose which version to keep.";
    default:
      return code;
  }
}

// --- Phase E: devices and enrollment (§5, §11.4) ---------------------------

export interface VaultDevice {
  device_id: string;
  device_name: string;
  platform: number;
  revoked: boolean;
  installed_seq: number;
  /// Unix seconds from the registry entry that enrolled it.
  enrolled_at: number | null;
  self: boolean;
}

export interface EnrollmentStart {
  qr: string;
  host: string;
  port: number;
  fp: string;
  expires_in: number;
}

export interface EnrollmentStatus {
  active: boolean;
  /** The 8-character code to compare with the phone's screen (§5.2). */
  sas: string | null;
  acked: boolean;
  expires_in?: number;
}

export async function vaultListDevices(): Promise<VaultDevice[]> {
  const resp = await invoke<{ devices: VaultDevice[] }>("vault_list_devices");
  return resp.devices ?? [];
}

/** Show the QR: ephemeral TLS server + single-use secret, 5-minute life. */
export async function vaultBeginEnrollment(): Promise<EnrollmentStart> {
  return await invoke<EnrollmentStart>("vault_begin_enrollment");
}

export async function vaultEnrollmentStatus(): Promise<EnrollmentStatus> {
  return await invoke<EnrollmentStatus>("vault_enrollment_status");
}

/** Only after the codes match on both screens. */
export async function vaultConfirmEnrollment(): Promise<void> {
  await invoke("vault_confirm_enrollment");
}

export async function vaultCancelEnrollment(): Promise<void> {
  await invoke("vault_cancel_enrollment");
}

/** Revoking rotates the vault key and issues a new Recovery Key. */
export async function vaultRevokeDevice(deviceId: string): Promise<void> {
  await invoke("vault_revoke_device", { deviceId });
}
