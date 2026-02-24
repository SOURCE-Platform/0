import { useState, useEffect, useRef } from "react";
import Settings from "./components/Settings";
import ScreenRecorder from "./components/ScreenRecorder";
import Recordings from "./components/Recordings";
import Hardware from "./components/Hardware";
import { ThemeProvider } from "./components/theme-provider";
import { UIPrefsProvider } from "./components/ui-prefs-provider";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";

type View = "settings" | "recorder" | "recordings" | "hardware";

const TABS = [
  { value: "recorder",   label: "Screen Recorder" },
  { value: "recordings", label: "Recordings" },
  { value: "hardware",   label: "Hardware" },
  { value: "settings",   label: "Settings" },
];

function App() {
  const [activeTab, setActiveTab] = useState<View>("recorder");
  const [displayedView, setDisplayedView] = useState<View>("recorder");
  const [fading, setFading] = useState(false);
  const fadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

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
        <div className="h-screen overflow-hidden bg-background">
          <div className="fixed top-0 inset-x-0 z-50 flex h-14 items-center px-6 gap-6 bg-background">
            <svg width="24" height="24" viewBox="0 0 28 28" className="fill-black dark:fill-white shrink-0">
              <circle cx="14" cy="14" r="14"/>
            </svg>
            <AnimatedTabNav
              tabs={TABS}
              value={activeTab}
              onValueChange={(v) => handleTabClick(v as View)}
            />
          </div>

          <main
            className="mt-14 h-[calc(100%-3.5rem)] overflow-y-auto mr-2 transition-opacity duration-200"
            style={{ opacity: fading ? 0 : 1 }}
          >
            <div className="container mx-auto px-6 py-6">
              {displayedView === "recorder"   && <ScreenRecorder />}
              {displayedView === "recordings" && <Recordings />}
              {displayedView === "hardware"   && <Hardware />}
              {displayedView === "settings"   && <Settings />}
            </div>
          </main>
        </div>
      </UIPrefsProvider>
    </ThemeProvider>
  );
}

export default App;
