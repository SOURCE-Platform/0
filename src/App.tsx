import { useState, useEffect, useRef } from "react";
import type { CSSProperties } from "react";
import ConsentManager from "./components/ConsentManager";
import Settings from "./components/Settings";
import ScreenRecorder from "./components/ScreenRecorder";
import Recordings from "./components/Recordings";
import Hardware from "./components/Hardware";
import { ThemeProvider } from "./components/theme-provider";
import { UIPrefsProvider } from "./components/ui-prefs-provider";
import { cn } from "@/lib/utils";

type View = "consent" | "settings" | "recorder" | "recordings" | "hardware";

const TABS: { value: View; label: string }[] = [
  { value: "consent",    label: "Privacy & Consent" },
  { value: "recorder",   label: "Screen Recorder" },
  { value: "recordings", label: "Recordings" },
  { value: "hardware",   label: "Hardware" },
  { value: "settings",   label: "Settings" },
];

function App() {
  const [activeTab, setActiveTab] = useState<View>("consent");
  const [displayedView, setDisplayedView] = useState<View>("consent");
  const [fading, setFading] = useState(false);
  const fadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [tabAnim, setTabAnim] = useState<{ from: View; to: View } | null>(null);
  const tabAnimTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  function handleTabClick(view: View) {
    if (view === activeTab) return;

    setTabAnim({ from: activeTab, to: view });
    if (tabAnimTimer.current) clearTimeout(tabAnimTimer.current);
    tabAnimTimer.current = setTimeout(() => setTabAnim(null), 200);

    setActiveTab(view);

    if (fadeTimer.current) clearTimeout(fadeTimer.current);
    setFading(true);
    fadeTimer.current = setTimeout(() => {
      setDisplayedView(view);
      setFading(false);
    }, 200);
  }

  useEffect(() => () => {
    if (fadeTimer.current) clearTimeout(fadeTimer.current);
    if (tabAnimTimer.current) clearTimeout(tabAnimTimer.current);
  }, []);

  function getUnderlineStyle(tabValue: View): CSSProperties {
    if (!tabAnim) {
      return {
        transform: activeTab === tabValue ? "scaleX(1)" : "scaleX(0)",
        transformOrigin: "left",
      };
    }

    const fromIdx = TABS.findIndex(t => t.value === tabAnim.from);
    const toIdx   = TABS.findIndex(t => t.value === tabAnim.to);
    const goingRight = toIdx > fromIdx;

    if (tabValue === tabAnim.from) {
      // Shrinks: going right → left side eats away (origin: right)
      //          going left  → right side eats away (origin: left)
      return {
        transformOrigin: goingRight ? "right" : "left",
        animation: "tab-underline-exit 0.2s linear forwards",
      };
    }

    if (tabValue === tabAnim.to) {
      // Grows:   going right → grows left-to-right (origin: left)
      //          going left  → grows right-to-left (origin: right)
      return {
        transformOrigin: goingRight ? "left" : "right",
        animation: "tab-underline-enter 0.2s cubic-bezier(0.0, 0.0, 0.2, 1) forwards",
      };
    }

    return { transform: "scaleX(0)", transformOrigin: "left" };
  }

  return (
    <ThemeProvider defaultTheme="light" storageKey="observer-theme">
      <UIPrefsProvider>
        <div className="min-h-screen bg-background">
          <div className="flex h-14 items-center px-6 gap-6">
            <svg width="24" height="24" viewBox="0 0 28 28" className="fill-black dark:fill-white shrink-0">
              <circle cx="14" cy="14" r="14"/>
            </svg>
            <nav className="flex items-center gap-5" role="tablist">
              {TABS.map(tab => (
                <button
                  key={tab.value}
                  role="tab"
                  aria-selected={activeTab === tab.value}
                  onClick={() => handleTabClick(tab.value)}
                  className={cn(
                    "relative cursor-pointer text-sm font-normal transition-colors focus-visible:outline-none",
                    activeTab === tab.value
                      ? "text-foreground"
                      : "text-foreground/60 hover:text-foreground"
                  )}
                >
                  {tab.label}
                  <span
                    className="absolute -bottom-[5px] inset-x-0 h-px bg-foreground"
                    style={getUnderlineStyle(tab.value)}
                  />
                </button>
              ))}
            </nav>
          </div>

          <main
            className="container mx-auto px-6 py-6 transition-opacity duration-200"
            style={{ opacity: fading ? 0 : 1 }}
          >
            {displayedView === "consent"    && <ConsentManager />}
            {displayedView === "recorder"   && <ScreenRecorder />}
            {displayedView === "recordings" && <Recordings />}
            {displayedView === "hardware"   && <Hardware />}
            {displayedView === "settings"   && <Settings />}
          </main>
        </div>
      </UIPrefsProvider>
    </ThemeProvider>
  );
}

export default App;
