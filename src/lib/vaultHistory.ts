//! Retained-history ops (spec §22.4): tombstoned records and restore.

import { invoke } from "@tauri-apps/api/core";

export interface DeletedItem {
  ref: string;
  kind: string;
  title: string | null;
  revision_id: string;
}

export async function vaultListDeleted(): Promise<DeletedItem[]> {
  const resp = await invoke<{ items: DeletedItem[] }>("vault_list_deleted");
  return resp.items ?? [];
}

export async function vaultRestoreRevision(reference: string, revisionId: string): Promise<void> {
  await invoke("vault_restore_revision", { reference, revisionId });
}
