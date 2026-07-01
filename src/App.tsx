import { Component, ReactNode, useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertCircle, CheckCircle2 } from "lucide-react";
import Settings from "./components/Settings";
import TimelinePage from "./components/TimelinePage";
import Hardware from "./components/Hardware";
import ID from "./components/ID";
import { GazeCalibrationOverlayPage } from "@/components/settings/GazeCalibrationOverlayPage";
import { ThemeProvider } from "./components/theme-provider";
import { UIPrefsProvider } from "./components/ui-prefs-provider";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { OBSERVER_APP_TOAST_EVENT, OBSERVER_CONFIG_UPDATED_EVENT, ObserverAppToastDetail } from "@/lib/app-config-events";

type View = "settings" | "timeline" | "hardware" | "id";

const TABS = [
  { value: "timeline",  label: "Timeline" },
  { value: "hardware",  label: "Hardware" },
  { value: "id",        label: "ID" },
  { value: "settings",  label: "Settings" },
];

interface AppConfig {
  mock_data_mode: boolean;
}

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

function RealModePlaceholder({
  title,
  body,
}: {
  title: string;
  body: string;
}) {
  return (
    <div className="mx-auto max-w-3xl rounded-2xl border border-dashed border-border/70 bg-muted/20 px-8 py-16 text-center">
      <h2 className="text-2xl font-semibold tracking-tight text-foreground">{title}</h2>
      <p className="mx-auto mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
        {body}
      </p>
    </div>
  );
}

function App() {
  const appMode = new URLSearchParams(window.location.search).get("mode");
  const [activeTab, setActiveTab] = useState<View>("timeline");
  const [displayedView, setDisplayedView] = useState<View>("timeline");
  const [fading, setFading] = useState(false);
  const [mockDataMode, setMockDataMode] = useState(true);
  const [toasts, setToasts] = useState<AppToast[]>([]);
  const fadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const toastTimerIds = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map());

  useEffect(() => {
    let mounted = true;

    async function loadConfig() {
      try {
        const config = await invoke<AppConfig>("get_config");
        if (mounted) {
          setMockDataMode(config.mock_data_mode ?? true);
        }
      } catch {
        if (mounted) {
          setMockDataMode(true);
        }
      }
    }

    loadConfig();

    function handleConfigUpdated(event: Event) {
      const customEvent = event as CustomEvent<AppConfig>;
      setMockDataMode(customEvent.detail?.mock_data_mode ?? true);
    }

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

    window.addEventListener(OBSERVER_CONFIG_UPDATED_EVENT, handleConfigUpdated as EventListener);
    window.addEventListener(OBSERVER_APP_TOAST_EVENT, handleAppToast as EventListener);

    return () => {
      mounted = false;
      window.removeEventListener(OBSERVER_CONFIG_UPDATED_EVENT, handleConfigUpdated as EventListener);
      window.removeEventListener(OBSERVER_APP_TOAST_EVENT, handleAppToast as EventListener);
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
          <div className="pointer-events-none fixed right-6 top-6 z-[200] flex max-w-sm flex-col gap-3">
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
          <div className="fixed top-0 inset-x-0 z-50 flex h-14 items-center px-6 gap-6 bg-transparent">
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
          <div className="fixed left-0 top-14 bottom-0 z-40 w-14 flex flex-col items-start justify-end pl-6 pb-[18px]">
            <div className="w-6 h-6 rounded-full bg-neutral-600 flex items-center justify-center overflow-hidden shrink-0">
              <span className="text-[9px] font-medium text-white leading-none select-none">A</span>
            </div>
          </div>

          <main
            className="mt-14 ml-14 mr-0 h-[calc(100%-3.5rem)] overflow-y-auto transition-opacity duration-200"
            style={{ opacity: fading ? 0 : 1 }}
          >
            <div className="w-full px-2 py-4 xl:px-3">
              <ViewErrorBoundary>
                {displayedView === "timeline"  && <TimelinePage mockDataMode={mockDataMode} />}
                {displayedView === "hardware"  && (
                  mockDataMode ? (
                    <Hardware />
                  ) : (
                    <RealModePlaceholder
                      title="Hardware is in real mode"
                      body="Mock sensor topology is hidden right now. As real cameras, microphones, and environmental feeds come online, this view can be driven by live hardware inventory instead of the demo home network."
                    />
                  )
                )}
                {displayedView === "id"        && (
                  mockDataMode ? (
                    <ID />
                  ) : (
                    <RealModePlaceholder
                      title="Identity is in real mode"
                      body="The demo profile dataset is turned off. Connect real identity/profile sources to populate this view, or re-enable Mock Data Mode in Settings when you want the polished demo experience."
                    />
                  )
                )}
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
