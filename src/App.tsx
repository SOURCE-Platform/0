import { useState, useEffect } from "react";
import ConsentManager from "./components/ConsentManager";
import Settings from "./components/Settings";
import ScreenRecorder from "./components/ScreenRecorder";
import { ThemeProvider } from "./components/theme-provider";
import { ThemeToggle } from "./components/theme-toggle";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";

type View = "consent" | "settings" | "recorder";

function App() {
  const [currentView, setCurrentView] = useState<View>("consent");

  // Add a reset button for testing
  const resetTheme = () => {
    localStorage.removeItem("observer-theme");
    window.location.reload();
  };

  return (
    <ThemeProvider defaultTheme="light" storageKey="observer-theme">
      <div className="min-h-screen bg-background">
        <div className="border-b">
          <div className="flex h-16 items-center px-6">
            <div className="flex items-center gap-6 flex-1">
              <h2 className="text-xl font-semibold text-foreground">Observer</h2>
              <Tabs value={currentView} onValueChange={(value) => setCurrentView(value as View)} className="flex-1">
                <TabsList>
                  <TabsTrigger value="consent">Privacy & Consent</TabsTrigger>
                  <TabsTrigger value="recorder">Screen Recorder</TabsTrigger>
                  <TabsTrigger value="settings">Settings</TabsTrigger>
                </TabsList>
              </Tabs>
            </div>
            <div className="flex items-center gap-2">
              <Button 
                variant="outline" 
                size="sm" 
                onClick={resetTheme}
                className="text-xs"
              >
                Reset Theme
              </Button>
              <ThemeToggle />
            </div>
          </div>
        </div>

        <main className="container mx-auto py-6">
          {currentView === "consent" && <ConsentManager />}
          {currentView === "recorder" && <ScreenRecorder />}
          {currentView === "settings" && <Settings />}
        </main>
      </div>
    </ThemeProvider>
  );
}

export default App;
