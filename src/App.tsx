import { Component, ReactNode, useState, useEffect, useRef } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, CheckCircle2 } from "lucide-react";
import Settings from "./components/Settings";
import TimelinePage from "./components/TimelinePage";
import ID from "./components/ID";
import AgentsPage from "@/components/agents/AgentsPage";
import { GazeCalibrationOverlayPage } from "@/components/settings/GazeCalibrationOverlayPage";
import { ThemeProvider } from "./components/theme-provider";
import { UIPrefsProvider } from "./components/ui-prefs-provider";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { OBSERVER_APP_TOAST_EVENT, ObserverAppToastDetail } from "@/lib/app-config-events";

type View = "settings" | "timeline" | "id" | "agents";

const TABS = [
  { value: "timeline",  label: "Timeline" },
  { value: "id",        label: "ID" },
  { value: "agents",    label: "Agents" },
  { value: "settings",  label: "Settings" },
];

interface AppToast extends ObserverAppToastDetail {
  id: number;
}

class ViewErrorBoundary extends Component<
  { children: ReactNode },
  { error: Error | null }
> {
  constructor(props: { children: ReactNode }) {
    super(props);
    this.state = { error: null };
  }

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (this.state.error) {
      return (
        <div className="mx-auto max-w-4xl rounded-2xl border border-red-500/30 bg-red-950/15 px-6 py-6">
          <h2 className="text-2xl font-semibold tracking-tight text-foreground">
            SOURCE hit a view error
          </h2>
          <p className="mt-3 max-w-[60ch] text-sm leading-6 text-muted-foreground">
            A screen inside the app failed while rendering. The details
            below stay visible so the app does not collapse into a blank
            white surface.
          </p>
          <pre className="mt-5 overflow-x-auto rounded-xl border border-border/70 bg-black/20 p-4 text-xs leading-6 text-red-100">
{this.state.error.stack ?? `${this.state.error.name}: ${this.state.error.message}`}
          </pre>
        </div>
      );
    }

    return this.props.children;
  }
}

function App() {
  const appMode = new URLSearchParams(window.location.search).get("mode");
  const nativeTitleBarHeight =
    isTauri() && navigator.userAgent.includes("Mac") ? 28 : 0;
  const contentTop = nativeTitleBarHeight + 56;
  const [activeTab, setActiveTab] = useState<View>("timeline");
  const [displayedView, setDisplayedView] = useState<View>("timeline");
  const [fading, setFading] = useState(false);
  const [toasts, setToasts] = useState<AppToast[]>([]);
  const fadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const toastTimerIds = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map());

  useEffect(() => {
    function handleAppToast(event: Event) {
      const customEvent = event as CustomEvent<ObserverAppToastDetail>;
      const detail = customEvent.detail;
      if (!detail?.text) return;

      const id = Date.now() + Math.floor(Math.random() * 1000);
      setToasts((current) => [...current, { id, ...detail }]);

      const timerId = setTimeout(() => {
        setToasts((current) => current.filter((toast) => toast.id !== id));
        toastTimerIds.current.delete(id);
      }, 2800);

      toastTimerIds.current.set(id, timerId);
    }

    window.addEventListener(OBSERVER_APP_TOAST_EVENT, handleAppToast as EventListener);

    // Pill gear button: jump to O's dictation settings even when the
    // Settings view is not mounted (it consumes the flag on mount).
    let unlistenSettings: (() => void) | undefined;
    listen("dictation-open-settings", () => {
      try {
        localStorage.setItem("open-settings-tab", "dictation");
      } catch {
        // Storage unavailable: Settings still opens, just on its last tab.
      }
      handleTabClick("settings");
    }).then((stop) => {
      unlistenSettings = stop;
    });

    return () => {
      window.removeEventListener(OBSERVER_APP_TOAST_EVENT, handleAppToast as EventListener);
      unlistenSettings?.();
      toastTimerIds.current.forEach((timerId) => clearTimeout(timerId));
      toastTimerIds.current.clear();
    };
  }, []);

  function handleTabClick(view: View) {
    if (view === activeTab) return;
    setActiveTab(view);
    if (fadeTimer.current) clearTimeout(fadeTimer.current);
    setFading(true);
    fadeTimer.current = setTimeout(() => {
      setDisplayedView(view);
      setFading(false);
    }, 200);
  }

  useEffect(() => () => { if (fadeTimer.current) clearTimeout(fadeTimer.current); }, []);

  return (
    <ThemeProvider defaultTheme="light" storageKey="observer-theme">
      <UIPrefsProvider>
        {appMode === "gaze-calibration" ? (
          <GazeCalibrationOverlayPage />
        ) : (
        <div className="app-container h-screen overflow-hidden">
          {nativeTitleBarHeight > 0 && (
            <div
              data-tauri-drag-region
              className="fixed inset-x-0 top-0 z-[60]"
              style={{ height: nativeTitleBarHeight }}
            />
          )}
          <div
            className="pointer-events-none fixed right-6 z-[200] flex max-w-sm flex-col gap-3"
            style={{ top: nativeTitleBarHeight + 24 }}
          >
            {toasts.map((toast) => (
              <div
                key={toast.id}
                className={`pointer-events-auto rounded-2xl border px-4 py-3 shadow-lg backdrop-blur-sm transition-all ${
                  toast.type === "success"
                    ? "border-green-500/60 bg-green-950/95 text-green-50"
                    : "border-red-500/60 bg-red-950/95 text-red-50"
                }`}
              >
                <div className="flex items-start gap-3">
                  {toast.type === "success" ? (
                    <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" />
                  ) : (
                    <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
                  )}
                  <p className="text-sm leading-6">{toast.text}</p>
                </div>
              </div>
            ))}
          </div>

          {/* Header */}
          <div
            className="fixed inset-x-0 z-50 flex h-14 items-center gap-6 bg-transparent px-6"
            style={{ top: nativeTitleBarHeight }}
          >
            <svg width="24" height="24" viewBox="0 0 28 28" className="fill-white shrink-0">
              <circle cx="14" cy="14" r="14"/>
            </svg>
            <AnimatedTabNav
              tabs={TABS}
              value={activeTab}
              onValueChange={(v) => handleTabClick(v as View)}
            />
          </div>

          {/* Left sidebar — same width as header height (w-14 = 56px) */}
          <div
            className="fixed bottom-0 left-0 z-40 flex w-14 flex-col items-start justify-end pb-[18px] pl-6"
            style={{ top: contentTop }}
          >
            <div className="w-6 h-6 rounded-full bg-neutral-600 flex items-center justify-center overflow-hidden shrink-0">
              <span className="text-[9px] font-medium text-white leading-none select-none">A</span>
            </div>
          </div>

          <main
            className="app-scroll-region ml-14 mr-0 overflow-y-auto transition-opacity duration-200"
            style={{
              height: `calc(100% - ${contentTop}px)`,
              marginTop: contentTop,
              opacity: fading ? 0 : 1,
            }}
          >
            <div className="w-full px-2 py-4 xl:px-3">
              <ViewErrorBoundary>
                {displayedView === "timeline"  && <TimelinePage />}
                {displayedView === "id"        && <ID />}
                {displayedView === "agents"    && <AgentsPage />}
                {displayedView === "settings"  && <Settings />}
              </ViewErrorBoundary>
            </div>
          </main>
        </div>
        )}
      </UIPrefsProvider>
    </ThemeProvider>
  );
}

export default App;
