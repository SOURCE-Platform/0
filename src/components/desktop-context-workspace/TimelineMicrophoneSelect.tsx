import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, LoaderCircle, Mic } from "lucide-react";
import { Popover } from "radix-ui";
import { normalizeAudioConfig } from "@/components/settings/audioConfig";
import { AudioSourceMeter } from "@/components/settings/AudioSourceMeter";
import type { AudioInputSource, Config } from "@/components/settings/types";
import {
  broadcastConfigUpdate,
  showSettingsToast,
} from "@/components/settings/utils";
import { cn } from "@/lib/utils";

interface TimelineMicrophoneSelectProps {
  currentSourceName: string | null;
}

export function TimelineMicrophoneSelect({
  currentSourceName,
}: TimelineMicrophoneSelectProps) {
  const [open, setOpen] = useState(false);
  const [sources, setSources] = useState<AudioInputSource[]>([]);
  const [config, setConfig] = useState<Config | null>(null);
  const [selectedId, setSelectedId] = useState<string>();
  const [selectedName, setSelectedName] = useState<string>();
  const [loading, setLoading] = useState(false);
  const [switching, setSwitching] = useState(false);
  const switchingRef = useRef(false);
  const lastLoadErrorRef = useRef<string | null>(null);

  async function refreshSources(allowFallback = false) {
    setLoading(true);
    try {
      const [rawConfig, available] = await Promise.all([
        invoke<Config>("get_config"),
        invoke<AudioInputSource[]>("list_audio_input_sources"),
      ]);
      const nextConfig = normalizeAudioConfig(rawConfig);
      setConfig(nextConfig);
      setSources(available);
      const selected = available.find(
        (source) => source.sourceId === nextConfig.selected_audio_input_id,
      );
      const active = available.find((source) => source.name === currentSourceName);
      const sameName = available.find((source) => source.name === selectedName);
      const fallback =
        available.find((source) => source.isSystemDefault) ?? available[0];
      const resolved = selected ?? sameName ?? active ?? fallback;
      lastLoadErrorRef.current = null;
      setSelectedId(resolved?.sourceId);
      setSelectedName(resolved?.name);

      const disconnectedName = selectedName ?? currentSourceName;
      const wasDisconnected =
        allowFallback &&
        disconnectedName != null &&
        !available.some((source) => source.name === disconnectedName);
      if (wasDisconnected && fallback) {
        await applySource(
          fallback,
          `${disconnectedName} disconnected. Switched to ${fallback.name}.`,
        );
      }
    } catch (error) {
      // The source list polls every few seconds; only toast when the
      // failure message changes so a persistent error doesn't flicker.
      const text = `Could not load microphone inputs: ${error}`;
      if (lastLoadErrorRef.current !== text) {
        lastLoadErrorRef.current = text;
        showSettingsToast({ type: "error", text });
      }
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refreshSources(false);
  }, [currentSourceName]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      void refreshSources(true);
    }, 3000);
    return () => window.clearInterval(interval);
  }, [currentSourceName, selectedName]);

  async function applySource(source: AudioInputSource, successText: string) {
    if (switchingRef.current) return;
    switchingRef.current = true;
    setSwitching(true);
    try {
      const current = normalizeAudioConfig(await invoke<Config>("get_config"));
      const next = normalizeAudioConfig({
        ...current,
        selected_audio_input_id: source.sourceId,
        audio_microphone_enabled: true,
      });
      await invoke("update_config", { config: next });
      broadcastConfigUpdate(next);
      setConfig(next);
      setSelectedId(source.sourceId);
      setSelectedName(source.name);
      await invoke("restart_multimodal_capture");
      showSettingsToast({ type: "success", text: successText });
    } catch (error) {
      showSettingsToast({
        type: "error",
        text: `Could not change microphone: ${error}`,
      });
    } finally {
      switchingRef.current = false;
      setSwitching(false);
    }
  }

  async function selectSource(sourceId: string) {
    if (switchingRef.current || sourceId === selectedId) return;
    const source = sources.find((item) => item.sourceId === sourceId);
    if (!source) return;
    await applySource(source, `Microphone changed to ${source.name}.`);
  }

  const triggerLabel = selectedName ?? currentSourceName
    ? `Microphone: ${selectedName ?? currentSourceName}`
    : "Choose microphone";

  return (
    <Popover.Root
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (nextOpen) {
          void refreshSources(true);
        } else {
          void invoke("stop_audio_meter_stream");
        }
      }}
    >
      <Popover.Trigger asChild>
        <button
          type="button"
          aria-label={triggerLabel}
          title={triggerLabel}
          className="flex size-8 cursor-pointer items-center justify-center rounded-md text-muted-foreground transition-colors hover:text-foreground focus-visible:bg-accent focus-visible:text-foreground focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50 data-[state=open]:text-foreground"
          disabled={switching}
        >
          <Mic className="size-4" />
        </button>
      </Popover.Trigger>

      <Popover.Portal>
        <Popover.Content
          align="end"
          sideOffset={8}
          className="z-50 w-72 rounded-lg border border-border bg-popover p-1 text-popover-foreground shadow-md outline-none"
        >
          <div className="flex items-center justify-between gap-3 px-2 py-1.5">
            <span className="text-xs font-medium text-muted-foreground">
              Microphone
            </span>
            <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
              {loading ? <LoaderCircle className="size-3 animate-spin" /> : null}
              {sources.length} available
            </span>
          </div>

          <div className="max-h-52 overflow-y-auto py-1" role="menu">
            {sources.length === 0 ? (
              <div className="px-2 py-3 text-xs text-muted-foreground">
                {loading ? "Finding microphones…" : "No microphones available"}
              </div>
            ) : (
              sources.map((source) => {
                const selected = source.sourceId === selectedId;
                return (
                  <button
                    key={source.sourceId}
                    type="button"
                    role="menuitemradio"
                    aria-checked={selected}
                    title={source.name}
                    onClick={() => void selectSource(source.sourceId)}
                    disabled={switching}
                    className={cn(
                      "flex w-full cursor-pointer items-center gap-2 rounded-md border-0 px-2 py-2 text-left text-sm outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground disabled:cursor-not-allowed disabled:opacity-50",
                      selected && "bg-accent/70",
                    )}
                  >
                    <span className="min-w-0 flex-1 truncate">{source.name}</span>
                    {source.isSystemDefault ? (
                      <span className="shrink-0 text-[10px] text-muted-foreground">
                        Default
                      </span>
                    ) : null}
                    <Check
                      className={cn("size-3.5 shrink-0", !selected && "invisible")}
                    />
                  </button>
                );
              })
            )}
          </div>

          {config ? (
            <AudioSourceMeter
              enabled
              selectedAudioInputId={selectedId ?? config.selected_audio_input_id}
              desktopAudioEnabled={config.audio_desktop_enabled}
              desktopGainDb={config.desktop_audio_gain_db}
              ownsStream={open}
              source="microphone"
              className="min-w-0 rounded-md border-x-0 border-b-0 bg-transparent"
            />
          ) : null}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
