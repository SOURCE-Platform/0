import { useState } from "react";
import ConsentManager from "./components/ConsentManager";
import Settings from "./components/Settings";
import ScreenRecorder from "./components/ScreenRecorder";
import Recordings from "./components/Recordings";
import { ThemeProvider } from "./components/theme-provider";
import { UIPrefsProvider } from "./components/ui-prefs-provider";
import { ThemeToggle } from "./components/theme-toggle";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

type View = "consent" | "settings" | "recorder" | "recordings";

function App() {
  const [currentView, setCurrentView] = useState<View>("consent");

  return (
    <ThemeProvider defaultTheme="light" storageKey="observer-theme">
      <UIPrefsProvider>
        <div className="min-h-screen bg-background">
          <div className="border-b">
            <div className="flex h-16 items-center px-6">
              <div className="flex items-center gap-6 flex-1">
                <svg width="28" height="28" viewBox="0 0 28 28" className="fill-black dark:fill-white"><circle cx="14" cy="14" r="14"/></svg>
                <Tabs value={currentView} onValueChange={(value) => setCurrentView(value as View)} className="flex-1">
                  <TabsList>
                    <TabsTrigger value="consent">Privacy & Consent</TabsTrigger>
                    <TabsTrigger value="recorder">Screen Recorder</TabsTrigger>
                    <TabsTrigger value="recordings">Recordings</TabsTrigger>
                    <TabsTrigger value="settings">Settings</TabsTrigger>
                  </TabsList>
                </Tabs>
              </div>
              <ThemeToggle />
            </div>
          </div>

          <main className="container mx-auto py-6">
            {currentView === "consent" && <ConsentManager />}
            {currentView === "recorder" && <ScreenRecorder />}
            {currentView === "recordings" && <Recordings />}
            {currentView === "settings" && <Settings />}
          </main>
        </div>
      </UIPrefsProvider>
    </ThemeProvider>
  );
}

export default App;
