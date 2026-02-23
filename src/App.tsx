import { useState, useEffect, useRef } from "react";
import ConsentManager from "./components/ConsentManager";
import Settings from "./components/Settings";
import ScreenRecorder from "./components/ScreenRecorder";
import Recordings from "./components/Recordings";
import { ThemeProvider } from "./components/theme-provider";
import { UIPrefsProvider } from "./components/ui-prefs-provider";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

type View = "consent" | "settings" | "recorder" | "recordings";

function App() {
  const [currentView, setCurrentView] = useState<View>("consent");
  const [displayedView, setDisplayedView] = useState<View>("consent");
  const [fading, setFading] = useState(false);
  const fadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  function handleViewChange(view: View) {
    if (view === currentView) return;
    if (fadeTimer.current) clearTimeout(fadeTimer.current);
    setCurrentView(view);
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
        <div className="min-h-screen bg-background">
          <div className="flex h-14 items-center px-6 gap-6">
            <svg width="24" height="24" viewBox="0 0 28 28" className="fill-black dark:fill-white shrink-0"><circle cx="14" cy="14" r="14"/></svg>
            <Tabs value={currentView} onValueChange={(value) => handleViewChange(value as View)}>
              <TabsList variant="line">
                <TabsTrigger value="consent">Privacy & Consent</TabsTrigger>
                <TabsTrigger value="recorder">Screen Recorder</TabsTrigger>
                <TabsTrigger value="recordings">Recordings</TabsTrigger>
                <TabsTrigger value="settings">Settings</TabsTrigger>
              </TabsList>
            </Tabs>
          </div>

          <main
            className="container mx-auto py-6 transition-opacity duration-200"
            style={{ opacity: fading ? 0 : 1 }}
          >
            {displayedView === "consent" && <ConsentManager />}
            {displayedView === "recorder" && <ScreenRecorder />}
            {displayedView === "recordings" && <Recordings />}
            {displayedView === "settings" && <Settings />}
          </main>
        </div>
      </UIPrefsProvider>
    </ThemeProvider>
  );
}

export default App;
