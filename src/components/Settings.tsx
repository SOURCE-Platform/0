import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Activity, Circle, CircleHelp, FolderOpen, HardDrive, Monitor, Moon, StopCircle, Sun, Trash2 } from "lucide-react";
import ConsentManager from "@/components/ConsentManager";
import { DISPLAY_KEY } from "@/components/TimelinePage";
import { useTheme } from "@/components/theme-provider";
import { useUIPrefs } from "@/components/ui-prefs-provider";
import { OBSERVER_APP_TOAST_EVENT, OBSERVER_CONFIG_UPDATED_EVENT, ObserverAppToastDetail } from "@/lib/app-config-events";
import { ChannelStatus, DesktopCaptureStatus } from "@/types/contextTimeline";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";

interface Display {
  id: number;
  name: string;
  width: number;
  height: number;
  is_primary: boolean;
}

interface CaptureChannels {
  system: boolean;
  focus: boolean;
  visible_windows: boolean;
  ocr: boolean;
  keyboard: boolean;
  mouse: boolean;
  screen_frames: boolean;
  audio_future: boolean;
  camera_future: boolean;
  sensor_future: boolean;
}

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
  mock_data_mode: boolean;
  capture_channels: CaptureChannels;
  resource_profile: "minimal" | "balanced" | "high_fidelity";
  pii_settings: {
    detect_only: boolean;
    enabled: boolean;
    enabled_categories: string[];
    review_confidence_threshold: number;
  };
}

interface CaptureDataChannelUsage {
  channel: string;
  label: string;
  storageKind: string;
  rowCount: number;
  diskBytes: number;
  lastEventTime: number | null;
}

interface CaptureDataOverview {
  databasePath: string;
  configuredStoragePath: string;
  actualRecordingsPath: string;
  databaseSizeBytes: number;
  recordingsSizeBytes: number;
  totalSizeBytes: number;
  diskTotalBytes: number;
  diskFreeBytes: number;
  diskUsedBytes: number;
  sourcePercentOfDisk: number;
  sourcePercentOfFreeSpace: number;
  diskHealth: string;
  diskWarning: string | null;
  channels: CaptureDataChannelUsage[];
  notes: string[];
}

interface CapturePreviewRow {
  timestamp: number | null;
  summary: string;
  rawJson: string;
}

interface CapturePreview {
  channel: string;
  label: string;
  rows: CapturePreviewRow[];
}

const CHANNEL_META: Array<{
  key: keyof CaptureChannels;
  label: string;
  description: string;
  help: string;
}> = [
  {
    key: "system",
    label: "OS / session events",
    description: "Capture-level system transitions and desktop session events.",
    help: "Records desktop lifecycle events such as capture start/stop, session transitions, app launches/quits, and other operating-system level context SOURCE can detect today.",
  },
  {
    key: "focus",
    label: "Focus and running apps",
    description: "Track the frontmost app and app-set changes over time.",
    help: "Tracks which app SOURCE believes is frontmost, plus snapshots of the current running-app set, so focus time can be separated from background presence.",
  },
  {
    key: "visible_windows",
    label: "Visible windows snapshots",
    description: "Best-effort scene context based on app snapshots.",
    help: "Builds a best-effort view of what was visible on screen at each scene sample. This is not a perfect historical window-server replay, but it helps reconstruct the desktop context around a moment.",
  },
  {
    key: "keyboard",
    label: "Keyboard activity",
    description: "Direct key activity for interactive-time classification.",
    help: "Records keyboard events so SOURCE can tell when you were actively typing rather than only passively viewing content.",
  },
  {
    key: "mouse",
    label: "Mouse activity",
    description: "Movement, clicks, and pointer-driven interaction.",
    help: "Records mouse movement and click activity so SOURCE can classify pointer-driven work separately from typing or passive viewing.",
  },
  {
    key: "ocr",
    label: "OCR text capture",
    description: "Text extraction from retained screen evidence when available.",
    help: "Runs text extraction over retained evidence so OCR Review and PII Review can show what readable text SOURCE captured from the screen.",
  },
  {
    key: "screen_frames",
    label: "Screen keyframes / evidence",
    description: "Retained frames and media anchors for review.",
    help: "Captures evidence frames from the selected display so timeline slices, OCR, and inspector views can point back to visual proof of what was on screen.",
  },
];

const RESOURCE_PROFILE_META: Record<
  Config["resource_profile"],
  {
    label: string;
    intervalSeconds: number;
    summary: string;
    details: string;
  }
> = {
  minimal: {
    label: "Minimal",
    intervalSeconds: 15,
    summary: "Lightest scene sampling, good for low-overhead validation.",
    details: "System, focus, and visible-window scene snapshots are sampled every 15 seconds while capture is running.",
  },
  balanced: {
    label: "Balanced",
    intervalSeconds: 5,
    summary: "Default desktop-context cadence for everyday capture.",
    details: "System, focus, and visible-window scene snapshots are sampled every 5 seconds while capture is running.",
  },
  high_fidelity: {
    label: "High Fidelity",
    intervalSeconds: 2,
    summary: "Fastest scene sampling for richer timeline reconstruction.",
    details: "System, focus, and visible-window scene snapshots are sampled every 2 seconds while capture is running.",
  },
};

const PII_CATEGORIES = [
  { value: "email", label: "Emails" },
  { value: "phone", label: "Phones" },
  { value: "government_id", label: "Government IDs" },
  { value: "credit_card", label: "Credit cards" },
  { value: "ip_address", label: "IP addresses" },
];

function formatTimestamp(value: number | null) {
  if (!value) return "No samples yet";
  return new Date(value).toLocaleString();
}

function formatBytes(bytes: number) {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}

function formatChannelActivity(status: ChannelStatus, isCaptureActive: boolean, isEnabled: boolean) {
  if (!isEnabled) {
    return "This channel is turned off.";
  }

  if (status.permissionState === "missing") {
    return "Permission is missing, so this channel cannot record yet.";
  }

  if (!isCaptureActive) {
    return "Capture is currently stopped. Start capture from the Timeline page to begin collecting data.";
  }

  if (status.sampleCount === 0) {
    return "Capture is running, but this channel has not produced a recent sample yet.";
  }

  const sampleLabel = status.sampleCount === 1 ? "sample" : "samples";
  return `${status.sampleCount} ${sampleLabel} recorded in the last hour (${status.throughputPerMinute.toFixed(2)}/min).`;
}

export default function Settings() {
  const [config, setConfig] = useState<Config | null>(null);
  const [displays, setDisplays] = useState<Display[]>([]);
  const [channelStatuses, setChannelStatuses] = useState<ChannelStatus[]>([]);
  const [captureStatus, setCaptureStatus] = useState<DesktopCaptureStatus | null>(null);
  const [dataOverview, setDataOverview] = useState<CaptureDataOverview | null>(null);
  const [previewChannel, setPreviewChannel] = useState("system");
  const [channelPreview, setChannelPreview] = useState<CapturePreview | null>(null);
  const [loadingPreview, setLoadingPreview] = useState(false);
  const [selectedDisplay, setSelectedDisplay] = useState(localStorage.getItem(DISPLAY_KEY) ?? "");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [deletingKey, setDeletingKey] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<{ channel: string | null; label: string } | null>(null);
  const [tab, setTab] = useState("general");
  const { theme, setTheme } = useTheme();
  const { showDescriptions, setShowDescriptions } = useUIPrefs();

  useEffect(() => {
    load();
  }, []);

  useEffect(() => {
    if (!captureStatus?.isActive) return;
    const interval = window.setInterval(() => {
      load(true);
    }, 5000);
    return () => window.clearInterval(interval);
  }, [captureStatus?.isActive]);

  useEffect(() => {
    if (tab !== "storage") return;
    void loadChannelPreview(previewChannel);
  }, [tab, previewChannel]);

  const selectedDisplayName = useMemo(
    () => displays.find((display) => String(display.id) === selectedDisplay)?.name ?? "No display selected",
    [displays, selectedDisplay],
  );

  async function load(silent = false) {
    if (!silent) {
      setLoading(true);
    }
    try {
      const [loadedConfig, availableDisplays, loadedChannelStatuses, loadedCaptureStatus, loadedDataOverview] = await Promise.all([
        invoke<Config>("get_config"),
        invoke<Display[]>("get_available_displays").catch(() => [] as Display[]),
        invoke<ChannelStatus[]>("get_channel_statuses").catch(() => [] as ChannelStatus[]),
        invoke<DesktopCaptureStatus>("get_desktop_capture_status").catch(() => null as DesktopCaptureStatus | null),
        invoke<CaptureDataOverview>("get_capture_data_overview").catch(() => null as CaptureDataOverview | null),
      ]);
      setConfig(loadedConfig);
      setDisplays(availableDisplays);
      setChannelStatuses(loadedChannelStatuses);
      setCaptureStatus(loadedCaptureStatus);
      setDataOverview(loadedDataOverview);

      const storedDisplayId = localStorage.getItem(DISPLAY_KEY);
      const matchingStoredDisplay = storedDisplayId
        ? availableDisplays.find((display) => String(display.id) === storedDisplayId)
        : null;
      if (!matchingStoredDisplay) {
        const primary = availableDisplays.find((display) => display.is_primary) ?? availableDisplays[0];
        if (primary) {
          localStorage.setItem(DISPLAY_KEY, String(primary.id));
          setSelectedDisplay(String(primary.id));
        } else {
          localStorage.removeItem(DISPLAY_KEY);
          setSelectedDisplay("");
        }
      }
    } catch (error) {
      showToast({ type: "error", text: `Failed to load settings: ${error}` });
    } finally {
      if (!silent) {
        setLoading(false);
      }
    }
  }

  function broadcastConfigUpdate(nextConfig: Config) {
    window.dispatchEvent(new CustomEvent(OBSERVER_CONFIG_UPDATED_EVENT, { detail: nextConfig }));
  }

  function showToast(detail: ObserverAppToastDetail) {
    window.dispatchEvent(new CustomEvent(OBSERVER_APP_TOAST_EVENT, { detail }));
  }

  function updateConfig(updates: Partial<Config>) {
    setConfig((previous) => (previous ? { ...previous, ...updates } : null));
  }

  function updateChannel(channel: keyof CaptureChannels, enabled: boolean) {
    if (!config) return;
    setConfig({
      ...config,
      capture_channels: {
        ...config.capture_channels,
        [channel]: enabled,
      },
    });
  }

  function updatePiiCategory(category: string, enabled: boolean) {
    if (!config) return;
    const next = enabled
      ? [...new Set([...config.pii_settings.enabled_categories, category])]
      : config.pii_settings.enabled_categories.filter((value) => value !== category);

    setConfig({
      ...config,
      pii_settings: {
        ...config.pii_settings,
        enabled_categories: next,
      },
    });
  }

  async function saveConfig(nextConfig = config) {
    if (!nextConfig) return;
    setSaving(true);
    try {
      await invoke("update_config", { config: nextConfig });
      broadcastConfigUpdate(nextConfig);
      await load(true);
      showToast({ type: "success", text: "Settings saved successfully." });
    } catch (error) {
      showToast({ type: "error", text: `Failed to save settings: ${error}` });
    } finally {
      setSaving(false);
    }
  }

  async function resetToDefaults() {
    setSaving(true);
    try {
      const defaults = await invoke<Config>("reset_config");
      setConfig(defaults);
      broadcastConfigUpdate(defaults);
      await load(true);
      showToast({ type: "success", text: "Settings reset to defaults." });
    } catch (error) {
      showToast({ type: "error", text: `Failed to reset settings: ${error}` });
    } finally {
      setSaving(false);
    }
  }

  async function handleMockDataModeChange(enabled: boolean) {
    if (!config) return;
    const nextConfig = { ...config, mock_data_mode: enabled };
    setConfig(nextConfig);
    await saveConfig(nextConfig);
  }

  async function enableOnlyChannel(channel: keyof CaptureChannels) {
    if (!config) return;
    const nextChannels = Object.keys(config.capture_channels).reduce((accumulator, key) => {
      accumulator[key as keyof CaptureChannels] = key === channel;
      return accumulator;
    }, {} as CaptureChannels);

    nextChannels.audio_future = false;
    nextChannels.camera_future = false;
    nextChannels.sensor_future = false;

    const nextConfig = {
      ...config,
      capture_channels: nextChannels,
    };
    setConfig(nextConfig);
    await saveConfig(nextConfig);
    showToast({
      type: "success",
      text: `Solo test mode is active for ${String(channel).split("_").join(" ")}.`,
    });
  }

  function applyResourceProfile(profile: Config["resource_profile"]) {
    updateConfig({ resource_profile: profile });
  }

  async function handleStartCapture() {
    try {
      const nextStatus = await invoke<DesktopCaptureStatus>("start_desktop_capture", {
        displayId: selectedDisplay ? Number(selectedDisplay) : null,
      });
      setCaptureStatus(nextStatus);
      await load(true);
      showToast({ type: "success", text: "Desktop capture started." });
    } catch (error) {
      showToast({ type: "error", text: `Failed to start capture: ${error}` });
    }
  }

  async function handleStopCapture() {
    try {
      const nextStatus = await invoke<DesktopCaptureStatus>("stop_desktop_capture");
      setCaptureStatus(nextStatus);
      await load(true);
      showToast({ type: "success", text: "Desktop capture stopped." });
    } catch (error) {
      showToast({ type: "error", text: `Failed to stop capture: ${error}` });
    }
  }

  async function handleRevealPath(target: "database" | "recordings" | "configured_storage") {
    try {
      await invoke("reveal_capture_data_target", { target });
    } catch (error) {
      showToast({ type: "error", text: `Failed to reveal path: ${error}` });
    }
  }

  function handleDeleteData(channel: string | null, label: string) {
    setPendingDelete({ channel, label });
  }

  async function confirmDeleteData() {
    if (!pendingDelete) return;

    const { channel, label } = pendingDelete;
    setDeletingKey(channel ?? "all");
    setPendingDelete(null);

    try {
      await invoke("delete_capture_data", { channel });
      await load(true);
      showToast({
        type: "success",
        text: channel ? `${label} data deleted.` : "All capture data deleted.",
      });
    } catch (error) {
      showToast({ type: "error", text: `Failed to delete data: ${error}` });
    } finally {
      setDeletingKey(null);
    }
  }

  async function loadChannelPreview(channel: string) {
    setLoadingPreview(true);
    try {
      const preview = await invoke<CapturePreview>("get_capture_channel_preview", {
        channel,
        limit: 12,
      });
      setChannelPreview(preview);
    } catch (error) {
      showToast({ type: "error", text: `Failed to load raw capture preview: ${error}` });
    } finally {
      setLoadingPreview(false);
    }
  }

  if (loading || !config) {
    return (
      <div className="flex min-h-[400px] items-center justify-center">
        <p className="text-muted-foreground">Loading settings...</p>
      </div>
    );
  }

  return (
    <div className="w-full max-w-6xl mx-auto space-y-6">
      <div className="space-y-1">
        <h1 className="text-3xl font-semibold tracking-tight text-foreground">Settings</h1>
        {showDescriptions && (
          <p className="text-muted-foreground">
            Configure real capture channels, demo mode, PII review behavior, and validation controls.
          </p>
        )}
      </div>

      <AnimatedTabNav
        tabs={[
          { value: "general", label: "General" },
          { value: "capture", label: "Capture" },
          { value: "privacy", label: "Privacy" },
          { value: "storage", label: "Storage" },
        ]}
        value={tab}
        onValueChange={setTab}
      />

      {tab === "general" && (
        <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>Data Mode</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="flex items-center gap-4">
                <span
                  className={`text-sm font-medium transition-colors ${
                    !config.mock_data_mode ? "text-foreground" : "text-muted-foreground"
                  }`}
                >
                  Real
                </span>
                <Switch
                  id="mock-mode"
                  checked={config.mock_data_mode}
                  onCheckedChange={handleMockDataModeChange}
                  disabled={saving}
                  className="data-[state=checked]:bg-blue-500 data-[state=unchecked]:bg-blue-500 dark:data-[state=unchecked]:bg-blue-500"
                />
                <span
                  className={`text-sm font-medium transition-colors ${
                    config.mock_data_mode ? "text-foreground" : "text-muted-foreground"
                  }`}
                >
                  Mock
                </span>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Application Behavior</CardTitle>
            </CardHeader>
            <CardContent className="space-y-6">
              <div className="flex items-center justify-between">
                <div className="space-y-1">
                  <Label htmlFor="auto-start" className="text-base">Launch on system startup</Label>
                  <p className="text-sm text-muted-foreground">Open SOURCE automatically when the computer boots.</p>
                </div>
                <Switch id="auto-start" checked={config.auto_start} onCheckedChange={(checked) => updateConfig({ auto_start: checked })} />
              </div>

              <div className="space-y-3">
                <Label className="text-base">Theme</Label>
                <div className="flex rounded-lg border overflow-hidden w-fit">
                  {[
                    { value: "light", label: "Light", icon: <Sun className="h-4 w-4" /> },
                    { value: "dark", label: "Dark", icon: <Moon className="h-4 w-4" /> },
                    { value: "system", label: "System", icon: <Monitor className="h-4 w-4" /> },
                  ].map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() => setTheme(option.value as "light" | "dark" | "system")}
                      className={`flex items-center gap-2 px-4 py-2 text-sm ${
                        theme === option.value ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:bg-muted"
                      }`}
                    >
                      {option.icon}
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="flex items-center justify-between">
                <div className="space-y-1">
                  <Label htmlFor="show-descriptions" className="text-base">Show descriptions</Label>
                  <p className="text-sm text-muted-foreground">Keep explanatory text visible throughout the app.</p>
                </div>
                <Switch id="show-descriptions" checked={showDescriptions} onCheckedChange={setShowDescriptions} />
              </div>
            </CardContent>
          </Card>
        </div>
      )}

      {tab === "capture" && (
        <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>Capture Preset</CardTitle>
              <CardDescription className="max-w-[60ch]">
                Choose how often SOURCE samples desktop context while
                capture is running.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-5">
              <div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_320px]">
                <div className="space-y-3">
                  <Label className="text-base">Resource Profile</Label>
                  <div className="inline-flex w-full overflow-hidden rounded-lg border border-border">
                    {(Object.keys(RESOURCE_PROFILE_META) as Array<Config["resource_profile"]>).map((profile) => {
                      const meta = RESOURCE_PROFILE_META[profile];
                      const selected = config.resource_profile === profile;
                      return (
                        <button
                          key={profile}
                          type="button"
                          onClick={() => applyResourceProfile(profile)}
                          className={`flex-1 px-4 py-3 text-sm font-medium transition-colors ${
                            selected
                              ? "bg-white/8 text-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.07)] ring-1 ring-white/18"
                              : "bg-muted/18 text-muted-foreground hover:bg-muted/28 hover:text-foreground"
                          }`}
                        >
                          {meta.label}
                        </button>
                      );
                    })}
                  </div>
                </div>

                <div className="space-y-2">
                  <Label className="text-base">Evidence Display</Label>
                  <Select
                    value={selectedDisplay}
                    onValueChange={(value) => {
                      setSelectedDisplay(value);
                      localStorage.setItem(DISPLAY_KEY, value);
                    }}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder="Select a display" />
                    </SelectTrigger>
                    <SelectContent>
                      {displays.map((display) => (
                        <SelectItem key={display.id} value={String(display.id)}>
                          {display.name} ({display.width}×{display.height}){display.is_primary ? " - Primary" : ""}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <p className="max-w-[36ch] text-xs text-muted-foreground">
                    Current screen evidence target: {selectedDisplayName}
                  </p>
                </div>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
              <div className="space-y-1.5">
                <CardTitle>Capture Channels</CardTitle>
                <CardDescription className="max-w-[60ch]">
                  When you press Start Capture, SOURCE begins a local
                  session using the enabled channels below and writes
                  data into the local database and recordings folder.
                </CardDescription>
              </div>
              <div className="flex flex-wrap items-center gap-3">
                <Badge variant={captureStatus?.isActive ? "default" : "secondary"}>
                  {captureStatus?.isActive ? "Running" : "Stopped"}
                </Badge>
                {!captureStatus?.isActive ? (
                  <Button
                    type="button"
                    size="sm"
                    className="gap-2"
                    onClick={handleStartCapture}
                    disabled={!selectedDisplay || saving}
                  >
                    <Circle className="h-3.5 w-3.5 fill-current" />
                    Start Capture
                  </Button>
                ) : (
                  <Button
                    type="button"
                    size="sm"
                    variant="destructive"
                    className="gap-2"
                    onClick={handleStopCapture}
                  >
                    <StopCircle className="h-3.5 w-3.5" />
                    Stop Capture
                  </Button>
                )}
              </div>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex flex-wrap items-center gap-3 rounded-xl border border-border/70 bg-muted/20 px-4 py-3 text-sm text-muted-foreground">
                <Activity className="h-4 w-4" />
                <span>
                  {captureStatus?.isActive
                    ? captureStatus.displayName
                      ? `Capturing on ${captureStatus.displayName}.`
                      : "Capture is running."
                    : "Capture is currently stopped."}
                </span>
                <span>
                  {captureStatus?.channelsEnabled.length ?? 0} channels enabled
                  {captureStatus?.startedAt ? ` · Started ${new Date(captureStatus.startedAt).toLocaleTimeString()}` : ""}
                </span>
              </div>

              <TooltipProvider>
                {CHANNEL_META.map((channel) => {
                  const status = channelStatuses.find((item) => item.channel === channel.key);
                  const isEnabled = config.capture_channels[channel.key];
                  const healthLabel = !isEnabled
                    ? "off"
                    : status?.health === "off"
                      ? "idle"
                      : status?.health ?? "warming_up";

                  return (
                    <div key={channel.key} className="rounded-xl border border-border/70 px-4 py-4">
                      <div className="flex flex-wrap items-start justify-between gap-6">
                        <div className="min-w-0 flex-1 space-y-2">
                          <div className="flex flex-wrap items-center gap-2">
                            <Label className="text-base">{channel.label}</Label>
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <button
                                  type="button"
                                  className="inline-flex h-5 w-5 items-center justify-center rounded-full text-muted-foreground transition-colors hover:text-foreground"
                                  aria-label={`Explain ${channel.label}`}
                                >
                                  <CircleHelp className="h-4 w-4" />
                                </button>
                              </TooltipTrigger>
                              <TooltipContent side="top" className="max-w-xs text-left leading-5">
                                {channel.help}
                              </TooltipContent>
                            </Tooltip>
                            {(status || !isEnabled) && (
                              <Badge variant={healthLabel === "healthy" ? "default" : healthLabel === "off" ? "secondary" : "outline"}>
                                {healthLabel}
                              </Badge>
                            )}
                          </div>
                          <p className="text-sm text-muted-foreground">{channel.description}</p>
                          {status && (
                            <div className="space-y-1 text-xs text-muted-foreground">
                              <div>
                                Permission: {status.permissionState} · Last sample: {formatTimestamp(status.lastEventTime)}
                              </div>
                              <div>{formatChannelActivity(status, captureStatus?.isActive ?? false, isEnabled)}</div>
                            </div>
                          )}
                        </div>

                        <div className="flex min-w-[240px] flex-col items-end gap-3">
                          <div className="flex items-center gap-3">
                            <span className="text-xs uppercase tracking-wide text-muted-foreground">
                              {isEnabled ? "On" : "Off"}
                            </span>
                            <Switch
                              checked={isEnabled}
                              onCheckedChange={(enabled) => updateChannel(channel.key, enabled)}
                            />
                          </div>
                          <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            onClick={() => enableOnlyChannel(channel.key)}
                            disabled={saving}
                          >
                            Solo test this channel
                          </Button>
                        </div>
                      </div>
                      {status?.lastError && (
                        <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-red-800 dark:bg-red-950/60 dark:text-red-100">
                          Last error: {status.lastError}
                        </div>
                      )}
                    </div>
                  );
                })}
              </TooltipProvider>
            </CardContent>
          </Card>
        </div>
      )}

      {tab === "privacy" && (
        <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>PII Detection</CardTitle>
              <CardDescription>
                Detect-only is the default. Review surfaces exist so you can inspect what SOURCE identified before any future automation is added.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-6">
              <div className="flex items-center justify-between">
                <div className="space-y-1">
                  <Label className="text-base">Enable PII detection</Label>
                  <p className="text-sm text-muted-foreground">Keep personal-data detection active in review flows.</p>
                </div>
                <Switch
                  checked={config.pii_settings.enabled}
                  onCheckedChange={(enabled) =>
                    updateConfig({
                      pii_settings: {
                        ...config.pii_settings,
                        enabled,
                      },
                    })
                  }
                />
              </div>

              <div className="flex items-center justify-between">
                <div className="space-y-1">
                  <Label className="text-base">Detect only</Label>
                  <p className="text-sm text-muted-foreground">Do not auto-redact or auto-drop content yet.</p>
                </div>
                <Switch
                  checked={config.pii_settings.detect_only}
                  onCheckedChange={(detect_only) =>
                    updateConfig({
                      pii_settings: {
                        ...config.pii_settings,
                        detect_only,
                      },
                    })
                  }
                />
              </div>

              <div className="space-y-3">
                <Label className="text-base">Entity categories</Label>
                <div className="grid gap-3 md:grid-cols-2">
                  {PII_CATEGORIES.map((category) => (
                    <div key={category.value} className="flex items-center justify-between rounded-lg border border-border/70 px-3 py-3">
                      <span className="text-sm">{category.label}</span>
                      <Switch
                        checked={config.pii_settings.enabled_categories.includes(category.value)}
                        onCheckedChange={(enabled) => updatePiiCategory(category.value, enabled)}
                      />
                    </div>
                  ))}
                </div>
              </div>

              <div className="space-y-2">
                <Label className="text-base">Review confidence threshold</Label>
                <Input
                  type="number"
                  min="0"
                  max="1"
                  step="0.05"
                  value={config.pii_settings.review_confidence_threshold}
                  onChange={(event) =>
                    updateConfig({
                      pii_settings: {
                        ...config.pii_settings,
                        review_confidence_threshold: Number.parseFloat(event.target.value) || 0,
                      },
                    })
                  }
                />
              </div>
            </CardContent>
          </Card>

          <ConsentManager />
        </div>
      )}

      {tab === "storage" && (
        <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>Storage Location</CardTitle>
              <CardDescription className="max-w-[60ch]">
                This is where SOURCE is currently writing local
                capture data.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <Input value={config.storage_path} readOnly />
              <Button
                type="button"
                variant="outline"
                className="gap-2"
                onClick={() => handleRevealPath("configured_storage")}
              >
                <FolderOpen className="h-4 w-4" />
                Open in Finder
              </Button>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Capture Data Access</CardTitle>
              <CardDescription className="max-w-[60ch]">
                SOURCE stores most channel data in one local SQLite
                database, with screen evidence files in a recordings
                folder beside it.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-5">
              <div className="rounded-xl border border-border/70 bg-muted/20 px-4 py-4">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <div className="text-sm font-medium text-foreground">Disk safety</div>
                    <p className="mt-1 max-w-[60ch] text-sm text-muted-foreground">
                      SOURCE footprint is compared against the remaining
                      free space on this disk.
                    </p>
                  </div>
                  <Badge
                    variant={
                      dataOverview?.diskHealth === "critical"
                        ? "destructive"
                        : dataOverview?.diskHealth === "warning"
                          ? "outline"
                          : "secondary"
                    }
                  >
                    {dataOverview?.diskHealth ?? "unknown"}
                  </Badge>
                </div>

                <div className="mt-4 grid gap-4 md:grid-cols-4">
                  <div>
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">SOURCE total</div>
                    <div className="mt-1 text-xl font-semibold text-foreground">
                      {formatBytes(dataOverview?.totalSizeBytes ?? 0)}
                    </div>
                  </div>
                  <div>
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">Disk free</div>
                    <div className="mt-1 text-xl font-semibold text-foreground">
                      {formatBytes(dataOverview?.diskFreeBytes ?? 0)}
                    </div>
                  </div>
                  <div>
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">Disk capacity</div>
                    <div className="mt-1 text-xl font-semibold text-foreground">
                      {formatBytes(dataOverview?.diskTotalBytes ?? 0)}
                    </div>
                  </div>
                  <div>
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">SOURCE vs free</div>
                    <div className="mt-1 text-xl font-semibold text-foreground">
                      {(dataOverview?.sourcePercentOfFreeSpace ?? 0).toFixed(1)}%
                    </div>
                  </div>
                </div>

                <div className="mt-4 space-y-3">
                  <div>
                    <div className="mb-1 flex items-center justify-between text-xs text-muted-foreground">
                      <span>SOURCE share of whole disk</span>
                      <span>{(dataOverview?.sourcePercentOfDisk ?? 0).toFixed(2)}%</span>
                    </div>
                    <div className="h-2 overflow-hidden rounded-full bg-white/8">
                      <div
                        className="h-full rounded-full bg-blue-500"
                        style={{ width: `${Math.min(dataOverview?.sourcePercentOfDisk ?? 0, 100)}%` }}
                      />
                    </div>
                  </div>
                  <div>
                    <div className="mb-1 flex items-center justify-between text-xs text-muted-foreground">
                      <span>Disk already used</span>
                      <span>
                        {dataOverview?.diskTotalBytes
                          ? `${((dataOverview.diskUsedBytes / dataOverview.diskTotalBytes) * 100).toFixed(1)}%`
                          : "0.0%"}
                      </span>
                    </div>
                    <div className="h-2 overflow-hidden rounded-full bg-white/8">
                      <div
                        className={`h-full rounded-full ${
                          dataOverview?.diskHealth === "critical"
                            ? "bg-red-500"
                            : dataOverview?.diskHealth === "warning"
                              ? "bg-amber-500"
                              : "bg-emerald-500"
                        }`}
                        style={{
                          width: `${
                            dataOverview?.diskTotalBytes
                              ? Math.min((dataOverview.diskUsedBytes / dataOverview.diskTotalBytes) * 100, 100)
                              : 0
                          }%`,
                        }}
                      />
                    </div>
                  </div>
                </div>

                {dataOverview?.diskWarning ? (
                  <div className="mt-4 rounded-lg border border-red-500/30 bg-red-950/30 px-3 py-3">
                    <p className="max-w-[60ch] text-sm text-red-100">{dataOverview.diskWarning}</p>
                  </div>
                ) : null}
              </div>

              <div className="grid gap-4 md:grid-cols-3">
                <div className="rounded-xl border border-border/70 bg-muted/20 px-4 py-4">
                  <div className="text-sm font-medium text-foreground">Database</div>
                  <div className="mt-2 text-2xl font-semibold text-foreground">
                    {formatBytes(dataOverview?.databaseSizeBytes ?? 0)}
                  </div>
                  <p className="mt-2 max-w-[32ch] text-xs text-muted-foreground">
                    SQLite file holding system, focus, OCR, keyboard,
                    mouse, and review metadata.
                  </p>
                </div>
                <div className="rounded-xl border border-border/70 bg-muted/20 px-4 py-4">
                  <div className="text-sm font-medium text-foreground">Recordings folder</div>
                  <div className="mt-2 text-2xl font-semibold text-foreground">
                    {formatBytes(dataOverview?.recordingsSizeBytes ?? 0)}
                  </div>
                  <p className="mt-2 max-w-[32ch] text-xs text-muted-foreground">
                    Screen keyframes, base layers, and encoded evidence
                    segments live here.
                  </p>
                </div>
                <div className="rounded-xl border border-border/70 bg-muted/20 px-4 py-4">
                  <div className="text-sm font-medium text-foreground">Total footprint</div>
                  <div className="mt-2 text-2xl font-semibold text-foreground">
                    {formatBytes(dataOverview?.totalSizeBytes ?? 0)}
                  </div>
                  <p className="mt-2 max-w-[32ch] text-xs text-muted-foreground">
                    Combined local storage currently used by SOURCE
                    capture data.
                  </p>
                </div>
              </div>

              <div className="grid gap-4">
                <div className="rounded-xl border border-border/70 px-4 py-4">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="space-y-1">
                      <div className="text-sm font-medium text-foreground">Database file</div>
                      <p className="max-w-[60ch] break-all text-xs text-muted-foreground">
                        {dataOverview?.databasePath ?? "Loading database path..."}
                      </p>
                    </div>
                    <Button type="button" variant="outline" size="sm" onClick={() => handleRevealPath("database")}>
                      Reveal in Finder
                    </Button>
                  </div>
                </div>

                <div className="rounded-xl border border-border/70 px-4 py-4">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="space-y-1">
                      <div className="text-sm font-medium text-foreground">Runtime recordings path</div>
                      <p className="max-w-[60ch] break-all text-xs text-muted-foreground">
                        {dataOverview?.actualRecordingsPath ?? "Loading recordings path..."}
                      </p>
                    </div>
                    <Button type="button" variant="outline" size="sm" onClick={() => handleRevealPath("recordings")}>
                      Reveal in Finder
                    </Button>
                  </div>
                </div>

                <div className="rounded-xl border border-border/70 px-4 py-4">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="space-y-1">
                      <div className="text-sm font-medium text-foreground">Configured storage path</div>
                      <p className="max-w-[60ch] break-all text-xs text-muted-foreground">
                        {dataOverview?.configuredStoragePath ?? config.storage_path}
                      </p>
                    </div>
                    <Button type="button" variant="outline" size="sm" onClick={() => handleRevealPath("configured_storage")}>
                      Reveal in Finder
                    </Button>
                  </div>
                </div>
              </div>

              {dataOverview?.notes.length ? (
                <div className="rounded-xl border border-border/70 bg-muted/20 px-4 py-4">
                  <div className="flex items-center gap-2 text-sm font-medium text-foreground">
                    <HardDrive className="h-4 w-4 text-muted-foreground" />
                    What to expect today
                  </div>
                  <div className="mt-3 space-y-2">
                    {dataOverview.notes.map((note) => (
                      <p key={note} className="max-w-[60ch] text-sm text-muted-foreground">
                        {note}
                      </p>
                    ))}
                  </div>
                </div>
              ) : null}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Channel Data Footprint</CardTitle>
              <CardDescription className="max-w-[60ch]">
                Review what each capture channel has stored so far and
                clear one channel at a time when needed.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              {(dataOverview?.channels ?? []).map((channel) => (
                <div key={channel.channel} className="rounded-xl border border-border/70 px-4 py-4">
                  <div className="flex flex-wrap items-start justify-between gap-4">
                    <div className="space-y-2">
                      <div className="text-base font-medium text-foreground">{channel.label}</div>
                      <div className="space-y-1 text-xs text-muted-foreground">
                        <div>Storage: {channel.storageKind}</div>
                        <div>Rows stored: {channel.rowCount.toLocaleString()}</div>
                        <div>Last sample: {formatTimestamp(channel.lastEventTime)}</div>
                        {channel.diskBytes > 0 ? <div>File footprint: {formatBytes(channel.diskBytes)}</div> : null}
                      </div>
                    </div>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="gap-2"
                      onClick={() => handleDeleteData(channel.channel, channel.label)}
                      disabled={captureStatus?.isActive || deletingKey !== null}
                    >
                      <Trash2 className="h-4 w-4" />
                      {deletingKey === channel.channel ? "Deleting..." : "Delete channel data"}
                    </Button>
                  </div>
                </div>
              ))}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Raw Capture Preview</CardTitle>
              <CardDescription className="max-w-[60ch]">
                Inspect the actual rows SOURCE is storing so you can see
                the raw captured structure, not just summary metrics.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="inline-flex w-full overflow-hidden rounded-lg border border-border md:w-auto">
                {(dataOverview?.channels ?? []).map((channel) => {
                  const selected = previewChannel === channel.channel;
                  return (
                    <button
                      key={channel.channel}
                      type="button"
                      onClick={() => setPreviewChannel(channel.channel)}
                      className={`px-3 py-2 text-sm transition-colors ${
                        selected
                          ? "bg-white/8 text-foreground ring-1 ring-white/18"
                          : "bg-muted/18 text-muted-foreground hover:bg-muted/28 hover:text-foreground"
                      }`}
                    >
                      {channel.label}
                    </button>
                  );
                })}
              </div>

              <div className="rounded-xl border border-border/70 bg-muted/20 px-4 py-4">
                {loadingPreview ? (
                  <p className="text-sm text-muted-foreground">Loading raw capture preview...</p>
                ) : channelPreview?.rows.length ? (
                  <div className="space-y-4">
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div>
                        <div className="font-medium text-foreground">{channelPreview.label}</div>
                        <p className="max-w-[60ch] text-sm text-muted-foreground">
                          These are recent stored rows from the local database for this channel.
                        </p>
                      </div>
                      <Badge variant="outline">{channelPreview.rows.length} recent rows</Badge>
                    </div>

                    <div className="space-y-3">
                      {channelPreview.rows.map((row, index) => (
                        <details key={`${row.timestamp ?? "none"}-${index}`} className="rounded-lg border border-border/70 bg-background/30 px-3 py-3">
                          <summary className="list-none">
                            <div className="flex flex-wrap items-start justify-between gap-3">
                              <div className="space-y-1">
                                <div className="text-sm font-medium text-foreground">{row.summary}</div>
                                <p className="text-xs text-muted-foreground">{formatTimestamp(row.timestamp)}</p>
                              </div>
                              <span className="text-xs uppercase tracking-wide text-muted-foreground">View JSON</span>
                            </div>
                          </summary>
                          <pre className="mt-3 overflow-x-auto rounded-lg border border-border/60 bg-black/20 p-3 text-xs leading-5 text-foreground">
{row.rawJson}
                          </pre>
                        </details>
                      ))}
                    </div>
                  </div>
                ) : (
                  <p className="max-w-[60ch] text-sm text-muted-foreground">
                    No stored rows yet for this channel. Record some data and come back here to inspect the raw payloads.
                  </p>
                )}
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Retention</CardTitle>
              <CardDescription className="max-w-[60ch]">
                How long different capture artifacts are kept.
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4 md:grid-cols-2">
              {Object.entries(config.retention_days).map(([key, value]) => (
                <div key={key} className="space-y-2">
                  <Label className="text-base capitalize">{key.split("_").join(" ")}</Label>
                  <Input
                    type="number"
                    min="1"
                    value={value}
                    onChange={(event) =>
                      updateConfig({
                        retention_days: {
                          ...config.retention_days,
                          [key]: Number.parseInt(event.target.value, 10) || 1,
                        },
                      })
                    }
                  />
                </div>
              ))}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Delete All Capture Data</CardTitle>
              <CardDescription className="max-w-[60ch]">
                Remove every stored capture row and every retained
                evidence file from this local SOURCE installation.
              </CardDescription>
            </CardHeader>
            <CardContent className="flex flex-wrap items-center justify-between gap-3">
              <p className="max-w-[60ch] text-sm text-muted-foreground">
                Stop capture first, then use this if you want a clean
                slate across all channels.
              </p>
              <Button
                type="button"
                variant="destructive"
                className="gap-2"
                onClick={() => handleDeleteData(null, "all capture")}
                disabled={captureStatus?.isActive || deletingKey !== null}
              >
                <Trash2 className="h-4 w-4" />
                {deletingKey === "all" ? "Deleting..." : "Delete everything"}
              </Button>
            </CardContent>
          </Card>
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>Save Changes</CardTitle>
          <CardDescription>
            Channel controls, PII settings, and capture presets are saved to the local SOURCE configuration.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap items-center justify-between gap-3">
          <p className="text-sm text-muted-foreground">
            Current evidence display: {selectedDisplayName}. Save after changing capture profile or channel mix.
          </p>
          <div className="flex gap-3">
            <Button type="button" variant="outline" onClick={resetToDefaults} disabled={saving}>
              Reset to defaults
            </Button>
            <Button type="button" onClick={() => saveConfig()} disabled={saving}>
              {saving ? "Saving..." : "Save settings"}
            </Button>
          </div>
        </CardContent>
      </Card>

      {pendingDelete ? (
        <div className="fixed inset-0 z-[220] flex items-center justify-center bg-black/55 px-6 backdrop-blur-sm">
          <div className="w-full max-w-xl rounded-2xl border border-border/70 bg-card p-6 shadow-2xl">
            <h2 className="text-2xl font-semibold text-foreground">
              Confirm deletion
            </h2>
            <p className="mt-3 max-w-[60ch] text-sm leading-6 text-muted-foreground">
              {pendingDelete.channel
                ? `Delete all stored data for ${pendingDelete.label}? This cannot be undone.`
                : "Delete all capture data across every channel? This cannot be undone."}
            </p>
            <div className="mt-6 flex flex-wrap justify-end gap-3">
              <Button
                type="button"
                variant="outline"
                onClick={() => setPendingDelete(null)}
                disabled={deletingKey !== null}
              >
                Cancel
              </Button>
              <Button
                type="button"
                variant="destructive"
                onClick={confirmDeleteData}
                disabled={deletingKey !== null}
              >
                Delete
              </Button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
