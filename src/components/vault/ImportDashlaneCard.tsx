//! Dashlane import (Phase I, spec §10.5–§10.6): one button, the report,
//! and the honest offer to move the export to the Trash. The export is a
//! plain-text file; the copy below says what deleting it can and cannot do.

import { useState } from "react";
import { FileUp, Trash2 } from "lucide-react";
import { vaultErrorMessage } from "@/lib/vault";
import { reportText, vaultImportDashlane, vaultTrashExport, type ImportReport } from "@/lib/vaultImport";

export function ImportDashlaneCard({ onImported, onError }: { onImported: () => void; onError: (m: string) => void }) {
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [trashed, setTrashed] = useState<"no" | "yes" | "failed">("no");

  async function start() {
    setBusy(true);
    try {
      const r = await vaultImportDashlane();
      if (r) {
        setReport(r);
        setTrashed("no");
        onImported();
      }
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    } finally {
      setBusy(false);
    }
  }

  async function trash() {
    try {
      setTrashed((await vaultTrashExport()) ? "yes" : "failed");
    } catch {
      setTrashed("failed");
    }
  }

  return (
    <div className="rounded-2xl border border-border/70 bg-black/10 p-6">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h2 className="text-lg font-medium text-foreground">Import from Dashlane</h2>
          <p className="mt-1 max-w-[60ch] text-sm leading-6 text-muted-foreground">
            Choose the CSV or ZIP that Dashlane exported. Logins, cards and secure notes are
            encrypted one by one as they are read. Card security codes and one-time-code
            secrets are not imported.
          </p>
        </div>
        <button
          onClick={start}
          disabled={busy}
          className="flex shrink-0 cursor-pointer items-center gap-2 rounded-xl border border-border/70 px-4 py-2 text-sm text-foreground transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <FileUp className="h-4 w-4" /> {busy ? "Importing…" : "Choose export…"}
        </button>
      </div>
      {report && (
        <div className="mt-4 space-y-3">
          <pre className="whitespace-pre-wrap rounded-lg border border-border/50 bg-black/20 px-3 py-2 text-xs leading-5 text-foreground">
            {reportText(report)}
          </pre>
          <button
            onClick={() => navigator.clipboard.writeText(reportText(report)).catch(() => {})}
            className="cursor-pointer rounded-lg border border-border/70 px-3 py-1.5 text-xs text-foreground hover:bg-white/5"
          >
            Copy report
          </button>
          <div className="rounded-xl border border-amber-300/40 bg-amber-950/10 p-4">
            <p className="max-w-[65ch] text-sm leading-6 text-amber-100">
              The export file is not encrypted. Anyone or any app that can read your files on
              this Mac can read every password in it, so don't open it in a text editor or
              spreadsheet. It may also already be copied to iCloud (Desktop, Documents or
              Downloads) or to backups such as Time Machine.
            </p>
            {report.keep_export ? (
              <p className="mt-3 max-w-[65ch] text-sm leading-6 text-amber-100">
                <strong>Keep the export for now.</strong> Some of it did not come in (see the
                report above — for example skipped rows, one-time-code secrets, or parts of the
                export this version can't read). Move those items into the vault by hand first;
                the file may be your only copy of them.
              </p>
            ) : (
              <>
                <p className="mt-3 max-w-[65ch] text-sm leading-6 text-amber-100">
                  Everything came in. Delete the export here and in any cloud or backup copy, and
                  empty the Trash. Even then, Source cannot promise the file is gone for good: the
                  Mac's storage can keep old copies that no app can reliably erase.
                </p>
                {trashed === "yes" ? (
                  <p className="mt-3 text-sm text-green-300">Moved to the Trash. Remember to empty the Trash.</p>
                ) : (
                  <button
                    onClick={trash}
                    className="mt-3 flex cursor-pointer items-center gap-2 rounded-lg border border-amber-300/50 px-3 py-1.5 text-xs text-amber-200 hover:bg-white/5"
                  >
                    <Trash2 className="h-3.5 w-3.5" /> Move the export to the Trash
                  </button>
                )}
                {trashed === "failed" && (
                  <p className="mt-2 text-sm text-red-300">Could not move it. Delete the file in Finder.</p>
                )}
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

export default ImportDashlaneCard;
