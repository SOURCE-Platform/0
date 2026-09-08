import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import DesktopContextWorkspace from "./DesktopContextWorkspace";
import type { Display } from "@/components/settings/types";

export const DISPLAY_KEY = "selected-display-id";

export default function TimelinePage() {
  const [displayId, setDisplayId] = useState<number | null>(() => {
    const raw = localStorage.getItem(DISPLAY_KEY);
    const parsed = raw ? Number.parseInt(raw, 10) : NaN;
    return Number.isNaN(parsed) ? null : parsed;
  });

  // Always-on vision: with no display chosen yet, adopt the first
  // available one so capture (and the audio pipeline with it) can start
  // without a settings detour.
  useEffect(() => {
    if (displayId !== null) return;
    let cancelled = false;
    invoke<Display[]>("get_available_displays")
      .then((displays) => {
        const first = displays[0];
        if (!cancelled && first && Number.isFinite(first.id)) {
          localStorage.setItem(DISPLAY_KEY, String(first.id));
          setDisplayId(first.id);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [displayId]);

  return (
    <div className="w-full">
      <DesktopContextWorkspace displayId={displayId} />
    </div>
  );
}
