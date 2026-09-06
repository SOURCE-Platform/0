import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DISPLAY_KEY } from "@/components/TimelinePage";
import { normalizeAudioConfig, updateChannelConfig } from "@/components/settings/audioConfig";
import { createAudioSettingsActions } from "@/components/settings/audioSettingsActions";
import { useTheme } from "@/components/theme-provider";
import { useUIPrefs } from "@/components/ui-prefs-provider";
import { ChannelStatus, DesktopCaptureStatus } from "@/types/contextTimeline";
import {
  AudioInputSource,
  CaptureChannels,
  CaptureDataOverview,
  CapturePreview,
  Config,
  Display,
  SettingsTab,
} from "@/components/settings/types";
import { broadcastConfigUpdate, showSettingsToast } from "@/components/settings/utils";

export function useSettingsController() {
  const [config, setConfig] = useState<Config | null>(null);
  const [displays, setDisplays] = useState<Display[]>([]);
  const [audioInputSources, setAudioInputSources] = useState<AudioInputSource[]>([]);
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
  const [tab, setTab] = useState<SettingsTab>("general");
  const { theme, setTheme } = useTheme();
  const { showDescriptions, setShowDescriptions } = useUIPrefs();

  useEffect(() => {
    void load();
  }, []);

  useEffect(() => {
    if (!captureStatus?.isActive) return;
    const interval = window.setInterval(() => {
      void load(true);
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
    if (!silent) setLoading(true);
    try {
      const [
        loadedConfig,
        availableDisplays,
        loadedAudioInputSources,
        loadedChannelStatuses,
        loadedCaptureStatus,
        loadedDataOverview,
      ] =
        await Promise.all([
          invoke<Config>("get_config"),
          invoke<Display[]>("get_available_displays").catch(() => [] as Display[]),
          invoke<AudioInputSource[]>("list_audio_input_sources").catch(
            () => [] as AudioInputSource[],
          ),
          invoke<ChannelStatus[]>("get_channel_statuses").catch(() => [] as ChannelStatus[]),
          invoke<DesktopCaptureStatus>("get_desktop_capture_status").catch(
            () => null as DesktopCaptureStatus | null,
          ),
          invoke<CaptureDataOverview>("get_capture_data_overview").catch(
            () => null as CaptureDataOverview | null,
          ),
        ]);

      setConfig(normalizeAudioConfig(loadedConfig));
      setDisplays(availableDisplays);
      setAudioInputSources(loadedAudioInputSources);
      setChannelStatuses(loadedChannelStatuses);
      setCaptureStatus(loadedCaptureStatus);
      setDataOverview(loadedDataOverview);

      const storedDisplayId = localStorage.getItem(DISPLAY_KEY);
      const matchingDisplay = storedDisplayId
        ? availableDisplays.find((display) => String(display.id) === storedDisplayId)
        : null;

      if (!matchingDisplay) {
        const primaryDisplay = availableDisplays.find((display) => display.is_primary) ?? availableDisplays[0];
        if (primaryDisplay) {
          localStorage.setItem(DISPLAY_KEY, String(primaryDisplay.id));
          setSelectedDisplay(String(primaryDisplay.id));
        } else {
          localStorage.removeItem(DISPLAY_KEY);
          setSelectedDisplay("");
        }
      }
    } catch (error) {
      showSettingsToast({ type: "error", text: `Failed to load settings: ${error}` });
    } finally {
      if (!silent) setLoading(false);
    }
  }

  function updateConfig(updates: Partial<Config>) {
    setConfig((previous) => (previous ? { ...previous, ...updates } : null));
  }

  function updateChannel(channel: keyof CaptureChannels, enabled: boolean) {
    if (!config) return;
    setConfig(updateChannelConfig(config, channel, enabled));
  }

  function updatePiiCategory(category: string, enabled: boolean) {
    if (!config) return;
    const nextCategories = enabled
      ? [...new Set([...config.pii_settings.enabled_categories, category])]
      : config.pii_settings.enabled_categories.filter((value) => value !== category);

    setConfig({
      ...config,
      pii_settings: {
        ...config.pii_settings,
        enabled_categories: nextCategories,
      },
    });
  }

  async function persistConfig(nextConfig: Config) {
    await invoke("update_config", { config: nextConfig });
    broadcastConfigUpdate(nextConfig);
  }

  async function saveConfig(nextConfig = config) {
    if (!nextConfig) return;
    setSaving(true);
    try {
      await persistConfig(nextConfig);
      await load(true);
      showSettingsToast({ type: "success", text: "Settings saved successfully." });
    } catch (error) {
      showSettingsToast({ type: "error", text: `Failed to save settings: ${error}` });
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
      showSettingsToast({ type: "success", text: "Settings reset to defaults." });
    } catch (error) {
      showSettingsToast({ type: "error", text: `Failed to reset settings: ${error}` });
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

    const nextConfig = { ...config, capture_channels: nextChannels };
    setConfig(nextConfig);
    await saveConfig(nextConfig);
    showSettingsToast({
      type: "success",
      text: `Solo test mode is active for ${String(channel).split("_").join(" ")}.`,
    });
  }

  function applyResourceProfile(profile: Config["resource_profile"]) {
    updateConfig({ resource_profile: profile });
  }

  async function handleStartCapture() {
    if (!config) return;
    try {
      const nextConfig = normalizeAudioConfig(config);
      setConfig(nextConfig);
      await persistConfig(nextConfig);
      const nextStatus = await invoke<DesktopCaptureStatus>("start_desktop_capture", {
        displayId: selectedDisplay ? Number(selectedDisplay) : null,
      });
      setCaptureStatus(nextStatus);
      await load(true);
      showSettingsToast({ type: "success", text: "Desktop capture started." });
    } catch (error) {
      showSettingsToast({ type: "error", text: `Failed to start capture: ${error}` });
    }
  }

  async function handleStopCapture() {
    try {
      const nextStatus = await invoke<DesktopCaptureStatus>("stop_desktop_capture");
      setCaptureStatus(nextStatus);
      await load(true);
      showSettingsToast({ type: "success", text: "Desktop capture stopped." });
    } catch (error) {
      showSettingsToast({ type: "error", text: `Failed to stop capture: ${error}` });
    }
  }

  async function handleRevealPath(target: "database" | "recordings" | "configured_storage") {
    try {
      await invoke("reveal_capture_data_target", { target });
    } catch (error) {
      showSettingsToast({ type: "error", text: `Failed to reveal path: ${error}` });
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
      showSettingsToast({
        type: "success",
        text: channel ? `${label} data deleted.` : "All capture data deleted.",
      });
    } catch (error) {
      showSettingsToast({ type: "error", text: `Failed to delete data: ${error}` });
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
      showSettingsToast({ type: "error", text: `Failed to load raw capture preview: ${error}` });
    } finally {
      setLoadingPreview(false);
    }
  }

  function selectDisplay(value: string) {
    setSelectedDisplay(value);
    localStorage.setItem(DISPLAY_KEY, value);
  }

  const audioActions = createAudioSettingsActions(config, setConfig);

  return {
    config,
    displays,
    audioInputSources,
    channelStatuses,
    captureStatus,
    dataOverview,
    previewChannel,
    channelPreview,
    loadingPreview,
    selectedDisplay,
    selectedDisplayName,
    loading,
    saving,
    deletingKey,
    pendingDelete,
    tab,
    theme,
    showDescriptions,
    setTheme,
    setShowDescriptions,
    setPreviewChannel,
    setTab,
    setPendingDelete,
    selectDisplay,
    ...audioActions,
    updateConfig,
    updateChannel,
    updatePiiCategory,
    saveConfig,
    resetToDefaults,
    handleMockDataModeChange,
    enableOnlyChannel,
    applyResourceProfile,
    handleStartCapture,
    handleStopCapture,
    handleRevealPath,
    handleDeleteData,
    confirmDeleteData,
  };
}

export type SettingsController = ReturnType<typeof useSettingsController>;
