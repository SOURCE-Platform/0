//! The vault is paused before its first real credential (owner decision
//! 2026-10-18). Shown at the top of the Vault tab in every state, with a
//! link to the pickup document a future session finishes from.

import { TriangleAlert } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

const DOC_PATH = "docs/security/vault-status-and-next-steps.md";
const DOC_URL = `https://github.com/SOURCE-Platform/0/blob/main/${DOC_PATH}`;

export function VaultUnfinishedBanner() {
  return (
    <div className="mt-4 flex items-start gap-3 rounded-2xl border border-amber-300/40 bg-amber-950/15 px-4 py-3">
      <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-amber-300" />
      <div className="max-w-[70ch] text-sm leading-6 text-amber-100">
        <p>
          <strong>The vault isn't finished.</strong> Development is paused before its final checks, so
          don't store real passwords, cards or notes here yet — keep using your current password
          manager. Use made-up test data only.
        </p>
        <button
          onClick={() => openUrl(DOC_URL).catch(() => {})}
          className="mt-1 cursor-pointer text-xs text-amber-200 underline underline-offset-2 hover:text-amber-100"
        >
          What's done and what's left ({DOC_PATH})
        </button>
      </div>
    </div>
  );
}

export default VaultUnfinishedBanner;
