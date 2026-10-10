// §15 user-facing copy for the vault's error codes (split from vault.ts).

/** Unlock only: a Mac whose Touch ID is unavailable (lid closed) gets
 * DEVICE_NOT_AUTHORIZED from its envelope; the master password works. */
export function vaultUnlockErrorMessage(code: string): string {
  return code === "DEVICE_NOT_AUTHORIZED"
    ? "Touch ID can't be used right now (for example, the lid is closed). Use your master password instead."
    : vaultErrorMessage(code);
}

/** §15 user-facing copy for the codes this surface can produce. */
export function vaultErrorMessage(code: string): string {
  if (code.startsWith("HELPER_UNAVAILABLE")) {
    return "The vault helper isn't available. Build and sign it (scripts/build-helper.sh) and try again.";
  }
  switch (code) {
    case "WRONG_CREDENTIAL":
      return "Incorrect password.";
    case "VAULT_BEHIND":
      return "This copy of your vault is older than one this device has already seen. It's read-only until it catches up.";
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
    case "DEVICE_NOT_AUTHORIZED":
      return "This Mac isn't authorized to do that right now.";
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
    case "MP_ADOPTION_REQUIRED":
      return "Another device changed your vault's security. Click “Back up now” and enter your master password to apply it.";
    case "CONFLICT_PENDING":
      return "This item was changed on two devices — choose which version to keep.";
    default:
      return code;
  }
}
