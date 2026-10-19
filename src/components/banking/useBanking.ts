import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { BankingOverview, TransactionPage, TransactionQuery } from "./types";

const emptyPage: TransactionPage = { rows: [], total: 0 };

export function useBanking() {
  const [overview, setOverview] = useState<BankingOverview | null>(null);
  const [page, setPage] = useState(emptyPage);
  const [query, setQuery] = useState<TransactionQuery>({ offset: 0, limit: 50 });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>("loading");
  const [protectedSurface, setProtectedSurface] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const alive = useRef(false);
  const request = useRef(0);

  useEffect(() => {
    alive.current = true;
    if (!isTauri()) { setBusy(null); return () => { alive.current = false; }; }
    let registered = false;
    let disposed = false;
    void (async () => {
      try {
        await invoke("sensitive_capture_surface_changed", { open: true });
        registered = true;
        if (disposed) {
          await invoke("sensitive_capture_surface_changed", { open: false });
          return;
        }
        setProtectedSurface(true);
        const data = await invoke<BankingOverview>("banking_overview");
        if (!disposed) setOverview(data);
      } catch (e) {
        if (!disposed) setError(String(e));
      } finally {
        if (!disposed) setBusy(null);
      }
    })();
    return () => {
      alive.current = false;
      disposed = true;
      if (registered) void invoke("sensitive_capture_surface_changed", { open: false }).catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!protectedSurface || !overview?.configured || busy) return;
    const id = ++request.current;
    const timer = setTimeout(() => {
      void invoke<TransactionPage>("banking_transactions", { query }).then((data) => {
        if (alive.current && request.current === id) setPage(data);
      }).catch((e) => {
        if (alive.current && request.current === id) setError(String(e));
      });
    }, 250);
    return () => { clearTimeout(timer); ++request.current; };
  }, [query, refreshKey, protectedSurface, overview?.configured, busy]);

  const run = useCallback(async (command: string, args: Record<string, unknown> = {}) => {
    if (!protectedSurface) return;
    setBusy(command);
    setError(null);
    try {
      const data = await invoke<BankingOverview>(command, args);
      if (alive.current) {
        setOverview(data);
        setRefreshKey((key) => key + 1);
      }
    } catch (e) {
      if (alive.current) {
        setError(String(e));
        // A connection can succeed while its first history import fails.
        try {
          const data = await invoke<BankingOverview>("banking_overview");
          if (alive.current) setOverview(data);
        } catch { /* Preserve the explicit error and last successful view. */ }
      }
    } finally {
      if (alive.current) setBusy(null);
    }
  }, [protectedSurface]);

  function filter(patch: Partial<TransactionQuery>) {
    setPage(emptyPage);
    setQuery((current) => ({ ...current, ...patch, offset: patch.offset ?? 0 }));
  }

  return {
    overview, page, query, filter, busy, error, protectedSurface, run,
    desktop: isTauri(),
    cancel: () => invoke("banking_cancel_connect").catch((e) => setError(String(e))),
  };
}
