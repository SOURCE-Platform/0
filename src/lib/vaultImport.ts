//! Dashlane import bindings (Phase I, spec §10.4–§10.6). The file picker
//! runs in the Rust side; this page only sees the report (counts,
//! categories, row numbers — never a value).

import { invoke } from "@tauri-apps/api/core";

export interface ImportReport {
  imported: number;
  duplicates: number;
  skipped_malformed: number;
  skipped_unsupported: Record<string, number>;
  cvv_discarded: number;
  skipped_rows: { file: string; row: number; why: string }[];
  rows_truncated: boolean;
  duration_ms: number;
}

/** Pick an export and import it; `null` when the picker was closed. */
export async function vaultImportDashlane(): Promise<ImportReport | null> {
  const resp = await invoke<{ cancelled?: boolean; report?: ImportReport }>("vault_import_dashlane");
  return resp.cancelled ? null : (resp.report ?? null);
}

/** Move the export just imported to the Trash (on the user's click). */
export async function vaultTrashExport(): Promise<boolean> {
  const resp = await invoke<{ ok: boolean }>("vault_trash_export");
  return resp.ok;
}

/** The report as plain text, for "Copy report". */
export function reportText(r: ImportReport): string {
  const lines = [
    `Imported: ${r.imported}`,
    `Already in the vault (kept as is): ${r.duplicates}`,
    `Skipped, unreadable rows: ${r.skipped_malformed}`,
    ...Object.entries(r.skipped_unsupported).map(([what, n]) => `Not imported — ${what}: ${n}`),
    ...(r.cvv_discarded ? [`Card security codes not imported (by design): ${r.cvv_discarded}`] : []),
    ...r.skipped_rows.map((s) => `  ${s.file} row ${s.row}: ${s.why}`),
    ...(r.rows_truncated ? ["  (more skipped rows not listed)"] : []),
    `Took ${(r.duration_ms / 1000).toFixed(1)} s`,
  ];
  return lines.join("\n");
}

/** Add one secure note (§10.8 Choice A). */
export async function vaultAddNote(title: string, body: string): Promise<void> {
  await invoke("vault_add_note", { title, body });
}
