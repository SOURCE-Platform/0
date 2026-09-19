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

export async function vaultSetup(): Promise<void> {
  await invoke("vault_setup");
}

export async function vaultUnlock(): Promise<void> {
  await invoke("vault_unlock");
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
    default:
      return code;
  }
}
