import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { Switch } from "@/components/ui/switch";
import { FolderOpen, AlertCircle, CheckCircle2, Sun, Moon, Monitor } from "lucide-react";
import ConsentManager from "@/components/ConsentManager";
import { useTheme } from "@/components/theme-provider";
import { useUIPrefs } from "@/components/ui-prefs-provider";

interface Config {
  storage_path: string;
  retention_days: Record<string, number>;
  recording_quality: "High" | "Medium" | "Low";
  auto_start: boolean;
  motion_detection_threshold: number;
  ocr_enabled: boolean;
  default_recording_fps: number;
  website_blacklist: string[];
  app_blacklist: string[];
}

export default function Settings() {
  const [config, setConfig] = useState<Config | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{ type: "success" | "error"; text: string } | null>(null);
  const [activeTab, setActiveTab] = useState("general");
  const [displayedTab, setDisplayedTab] = useState("general");
  const [tabFading, setTabFading] = useState(false);
  const fadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  function handleTabChange(tab: string) {
    if (tab === activeTab) return;
    if (fadeTimer.current) clearTimeout(fadeTimer.current);
    setActiveTab(tab);
    setTabFading(true);
    fadeTimer.current = setTimeout(() => {
      setDisplayedTab(tab);
      setTabFading(false);
    }, 200);
  }

  useEffect(() => () => { if (fadeTimer.current) clearTimeout(fadeTimer.current); }, []);
  const { theme, setTheme } = useTheme();
  const { showDescriptions, setShowDescriptions } = useUIPrefs();

  useEffect(() => {
    loadConfig();
  }, []);

  async function loadConfig() {
    try {
      const loadedConfig = await invoke<Config>("get_config");
      setConfig(loadedConfig);
    } catch (error) {
      console.error("Failed to load config:", error);
      setMessage({ type: "error", text: `Failed to load settings: ${error}` });
    } finally {
      setLoading(false);
    }
  }

  async function saveConfig() {
    if (!config) return;

    setSaving(true);
    setMessage(null);

    try {
      await invoke("update_config", { config });
      setMessage({ type: "success", text: "Settings saved successfully!" });
      setTimeout(() => setMessage(null), 3000);
    } catch (error) {
      console.error("Failed to save config:", error);
      setMessage({ type: "error", text: `Failed to save settings: ${error}` });
    } finally {
      setSaving(false);
    }
  }

  async function resetToDefaults() {
    setSaving(true);
    setMessage(null);

    try {
      const defaultConfig = await invoke<Config>("reset_config");
      setConfig(defaultConfig);
      setMessage({ type: "success", text: "Settings reset to defaults!" });
      setTimeout(() => setMessage(null), 3000);
    } catch (error) {
      console.error("Failed to reset config:", error);
      setMessage({ type: "error", text: `Failed to reset settings: ${error}` });
    } finally {
      setSaving(false);
    }
  }

  async function selectStoragePath() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });

      if (selected && typeof selected === "string") {
        updateConfig({ storage_path: selected });
      }
    } catch (error) {
      console.error("Failed to select directory:", error);
      setMessage({ type: "error", text: "Failed to open directory selector" });
    }
  }

  function updateConfig(updates: Partial<Config>) {
    setConfig((prev) => (prev ? { ...prev, ...updates } : null));
  }

  function updateRetentionDays(dataType: string, days: number) {
    if (!config) return;
    const newRetentionDays = { ...config.retention_days, [dataType]: days };
    updateConfig({ retention_days: newRetentionDays });
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-[400px]">
        <p className="text-muted-foreground">Loading settings...</p>
      </div>
    );
  }

  if (!config) {
    return (
      <div className="flex items-center justify-center min-h-[400px]">
        <p className="text-destructive">Failed to load settings. Please restart the application.</p>
      </div>
    );
  }

  return (
    <div className="w-full max-w-5xl mx-auto space-y-6">
      <div className="space-y-1">
        <h1 className="text-3xl font-semibold tracking-tight text-foreground">Settings</h1>
        {showDescriptions && (
          <p className="text-muted-foreground">Manage your application preferences and configuration</p>
        )}
      </div>

      {message && (
        <Card className={message.type === "success" ? "border-green-500 bg-green-50 dark:bg-green-950" : "border-red-500 bg-red-50 dark:bg-red-950"}>
          <CardContent className="pt-6">
            <div className="flex items-center gap-2">
              {message.type === "success" ? (
                <CheckCircle2 className="h-4 w-4 text-green-600 dark:text-green-400" />
              ) : (
                <AlertCircle className="h-4 w-4 text-red-600 dark:text-red-400" />
              )}
              <p className={message.type === "success" ? "text-green-900 dark:text-green-100" : "text-red-900 dark:text-red-100"}>
                {message.text}
              </p>
            </div>
          </CardContent>
        </Card>
      )}

      <div className="w-full">
        <AnimatedTabNav
          tabs={[
            { value: "general",   label: "General" },
            { value: "recording", label: "Recording" },
            { value: "storage",   label: "Storage" },
            { value: "privacy",   label: "Privacy" },
          ]}
          value={activeTab}
          onValueChange={handleTabChange}
        />

        <div
          className="transition-opacity duration-200 mt-6"
          style={{ opacity: tabFading ? 0 : 1 }}
        >

        {displayedTab === "general" && <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>General Settings</CardTitle>
              {showDescriptions && (
                <CardDescription>Configure application behavior and startup options</CardDescription>
              )}
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex items-center justify-between">
                <div className="space-y-0.5">
                  <Label htmlFor="auto-start" className="text-base">Launch on system startup</Label>
                  {showDescriptions && (
                    <p className="text-sm text-muted-foreground">
                      Automatically start Observer when your computer boots up
                    </p>
                  )}
                </div>
                <Switch
                  id="auto-start"
                  checked={config.auto_start}
                  onCheckedChange={(checked) => updateConfig({ auto_start: checked })}
                />
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>App UI</CardTitle>
              {showDescriptions && (
                <CardDescription>Appearance and information density</CardDescription>
              )}
            </CardHeader>
            <CardContent className="space-y-6">
              <div className="space-y-3">
                <Label className="text-base">Theme</Label>
                <div className="flex rounded-lg border overflow-hidden w-fit">
                  {[
                    { value: "light", label: "Light", icon: <Sun className="h-4 w-4" /> },
                    { value: "dark",  label: "Dark",  icon: <Moon className="h-4 w-4" /> },
                    { value: "system", label: "System", icon: <Monitor className="h-4 w-4" /> },
                  ].map(({ value, label, icon }) => (
                    <button
                      key={value}
                      onClick={() => setTheme(value as "light" | "dark" | "system")}
                      className={`cursor-pointer flex items-center gap-2 px-4 py-2 text-sm font-medium transition-colors ${
                        theme === value
                          ? "bg-primary text-primary-foreground"
                          : "hover:bg-muted text-muted-foreground hover:text-foreground"
                      }`}
                    >
                      {icon}
                      {label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="flex items-center justify-between">
                <div className="space-y-0.5">
                  <Label htmlFor="show-descriptions" className="text-base">Show descriptions</Label>
                  {showDescriptions && (
                    <p className="text-sm text-muted-foreground">
                      Display explanatory text and info panels throughout the app
                    </p>
                  )}
                </div>
                <Switch
                  id="show-descriptions"
                  checked={showDescriptions}
                  onCheckedChange={setShowDescriptions}
                />
              </div>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>Reset to Defaults</CardTitle>
              {showDescriptions && (
                <CardDescription>Restore all settings to their original values</CardDescription>
              )}
            </CardHeader>
            <CardContent className="flex items-center justify-between">
              <p className="text-sm text-muted-foreground">This will overwrite your current configuration.</p>
              <div className="flex gap-3">
                <Button onClick={resetToDefaults} disabled={saving} variant="outline">
                  Reset to Defaults
                </Button>
                <Button onClick={saveConfig} disabled={saving}>
                  {saving ? "Saving..." : "Save Settings"}
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>}

        {displayedTab === "recording" && <div className="grid grid-cols-2 gap-4">
          <Card>
            <CardHeader>
              <CardTitle>Recording Quality</CardTitle>
              {showDescriptions && <CardDescription>Higher quality produces clearer screenshots but uses more storage</CardDescription>}
            </CardHeader>
            <CardContent>
              <Select
                value={config.recording_quality}
                onValueChange={(value) => updateConfig({ recording_quality: value as Config["recording_quality"] })}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="High">High (Best quality, larger files)</SelectItem>
                  <SelectItem value="Medium">Medium (Balanced)</SelectItem>
                  <SelectItem value="Low">Low (Smaller files, lower quality)</SelectItem>
                </SelectContent>
              </Select>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Frames Per Second</CardTitle>
              {showDescriptions && <CardDescription>How many screenshots to capture per second (1-60)</CardDescription>}
            </CardHeader>
            <CardContent>
              <div className="flex items-center gap-4">
                <Input
                  type="number"
                  min="1"
                  max="60"
                  value={config.default_recording_fps}
                  onChange={(e) => updateConfig({ default_recording_fps: parseInt(e.target.value, 10) || 1 })}
                  className="w-24"
                />
                <span className="text-sm text-muted-foreground">FPS</span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Motion Detection Threshold</CardTitle>
              {showDescriptions && <CardDescription>Skip recording frames with less than this amount of change</CardDescription>}
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex items-center gap-4">
                <Slider
                  value={[config.motion_detection_threshold]}
                  onValueChange={(value) => updateConfig({ motion_detection_threshold: value[0] })}
                  min={0}
                  max={1}
                  step={0.01}
                  className="flex-1"
                />
                <span className="min-w-[4rem] text-sm font-medium text-right">
                  {(config.motion_detection_threshold * 100).toFixed(0)}%
                </span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>OCR (Text Recognition)</CardTitle>
              {showDescriptions && <CardDescription>Extract text from screenshots for searchability</CardDescription>}
            </CardHeader>
            <CardContent>
              <div className="flex items-center justify-between">
                <Label htmlFor="ocr-enabled">Enable OCR processing</Label>
                <Switch
                  id="ocr-enabled"
                  checked={config.ocr_enabled}
                  onCheckedChange={(checked) => updateConfig({ ocr_enabled: checked })}
                />
              </div>
            </CardContent>
          </Card>
          </div>}

        {displayedTab === "storage" && <div className="grid grid-cols-2 gap-4 items-start">
          <Card>
            <CardHeader>
              <CardTitle>Storage Location</CardTitle>
              {showDescriptions && <CardDescription>Where recordings and data are stored</CardDescription>}
            </CardHeader>
            <CardContent>
              <div className="flex flex-col gap-3">
                <Input value={config.storage_path} readOnly />
                <Button onClick={selectStoragePath} variant="outline" className="gap-2 w-full">
                  <FolderOpen className="h-4 w-4" />
                  Browse
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Data Retention</CardTitle>
              {showDescriptions && <CardDescription>How long to keep different types of data (in days)</CardDescription>}
            </CardHeader>
            <CardContent>
              <div className="grid gap-6 sm:grid-cols-2">
                {[
                  { key: "screen", label: "Screen Recordings", default: 30 },
                  { key: "ocr", label: "OCR Data", default: 90 },
                  { key: "keyboard", label: "Keyboard Activity", default: 30 },
                  { key: "mouse", label: "Mouse Activity", default: 7 },
                ].map(({ key, label, default: def }) => (
                  <div key={key} className="space-y-2">
                    <div className="flex items-center justify-between">
                      <Label>{label}</Label>
                      <span className="text-sm font-medium">{config.retention_days[key] ?? def} days</span>
                    </div>
                    <Slider
                      value={[config.retention_days[key] ?? def]}
                      onValueChange={(value) => updateRetentionDays(key, value[0])}
                      min={1}
                      max={365}
                      step={1}
                    />
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>
          </div>}

        {displayedTab === "privacy" && <ConsentManager />}

        </div>
      </div>
    </div>
  );
}
