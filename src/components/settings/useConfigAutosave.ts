import { useEffect, useRef } from "react";
import { Config } from "@/components/settings/types";

const AUTOSAVE_DELAY_MS = 400;

/**
 * Persists every user edit to the config automatically. Edits are debounced
 * so a burst (typing a number, dragging through toggles) writes once.
 *
 * Configs that came from the backend, or were just written to it, must be
 * passed to the returned `markSynced` before `setConfig` so they are not
 * written straight back.
 */
export function useConfigAutosave(
  config: Config | null,
  persist: (config: Config) => Promise<void>,
  onError: (error: unknown) => void,
) {
  const synced = useRef<Config | null>(null);

  useEffect(() => {
    if (!config || config === synced.current) return;
    const handle = window.setTimeout(() => {
      synced.current = config;
      persist(config).catch(onError);
    }, AUTOSAVE_DELAY_MS);
    return () => window.clearTimeout(handle);
  }, [config]);

  return (next: Config | null) => {
    synced.current = next;
  };
}
