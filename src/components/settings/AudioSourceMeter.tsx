import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AudioSourceMeters } from "@/components/settings/types";
import { cn } from "@/lib/utils";

const BAR_COUNT = 20;
const ATTACK = 0.62;
const RELEASE = 0.24;

interface AudioSourceMeterProps {
  className?: string;
  enabled: boolean;
  selectedAudioInputId?: string | null;
  desktopAudioEnabled: boolean;
  desktopGainDb: number;
  ownsStream?: boolean;
  source: "microphone" | "desktop";
}

export function AudioSourceMeter({
  className,
  enabled,
  selectedAudioInputId,
  desktopAudioEnabled,
  desktopGainDb,
  ownsStream = false,
  source,
}: AudioSourceMeterProps) {
  const [meters, setMeters] = useState<AudioSourceMeters | null>(null);
  const [startupError, setStartupError] = useState<string | null>(null);
  const [displayLevel, setDisplayLevel] = useState(0);
  const meter = meters?.[source] ?? null;
  const isActive = enabled && meter?.status === "active";
  const level = isActive ? Math.max(0, Math.min(1, meter.level)) : 0;
  const activeBars = displayLevel * BAR_COUNT;
  const label = getStatusLabel(enabled, meter, displayLevel);

  useEffect(() => {
    let frame = 0;
    const tick = () => {
      setDisplayLevel((current) => {
        const nextRate = level > current ? ATTACK : RELEASE;
        const next = current + (level - current) * nextRate;
        return Math.abs(next - level) < 0.01 ? level : next;
      });
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(frame);
  }, [level]);

  useEffect(() => {
    if (!enabled && !ownsStream) {
      setMeters(null);
      setStartupError(null);
      void invoke("stop_audio_meter_stream");
      return;
    }

    let cancelled = false;
    let unlisten: (() => void) | undefined;
    const connect = async () => {
      try {
        setMeters(null);
        setDisplayLevel(0);
        unlisten = await listen<AudioSourceMeters>("audio-meter-frame", (event) => {
          if (cancelled) return;
          setMeters(event.payload);
        });
        if (ownsStream) {
          const initial = await invoke<AudioSourceMeters>("start_audio_meter_stream", {
            selectedAudioInputId,
            desktopAudioEnabled,
            desktopAudioGainDb: desktopGainDb,
          });
          if (!cancelled) setMeters(initial);
        }
        if (!cancelled) setStartupError(null);
      } catch (error) {
        if (!cancelled) {
          setMeters(null);
          setStartupError(formatStartupError(error));
        }
      }
    };

    void connect();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [desktopAudioEnabled, desktopGainDb, enabled, ownsStream, selectedAudioInputId]);

  return (
    <div
      className={cn(
        "min-w-[11rem] rounded-lg border border-border/60 bg-background/35 px-3 py-2",
        className,
      )}
      title={meter?.message ?? `${source} signal level`}
    >
      <div className="flex items-center justify-between gap-3 text-xs">
        <span className="font-medium text-foreground">Signal level</span>
        <span className="text-muted-foreground">{startupError ? "Unavailable" : label}</span>
      </div>
      <div
        aria-label={`${source} signal level`}
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={Math.round(displayLevel * 100)}
        className="mt-2 flex h-8 items-end gap-0.5"
        role="meter"
      >
        {Array.from({ length: BAR_COUNT }, (_, index) => {
          const amount = Math.max(0, Math.min(1, activeBars - index));
          const active = amount > 0.03;
          return (
            <span
              key={index}
              className={cn(
                "w-1 rounded-full origin-bottom transition-[background-color,opacity,transform] duration-75",
                active ? "bg-blue-500" : "bg-muted",
              )}
              style={{
                height: `${24 + index * 3.6}%`,
                opacity: active ? 0.3 + amount * 0.7 : 0.42,
                transform: `scaleY(${active ? 0.35 + amount * 0.65 : 0.28})`,
              }}
            />
          );
        })}
      </div>
      {startupError || meter?.message ? (
        <p className="mt-2 max-w-[28ch] text-xs leading-4 text-muted-foreground">
          {startupError ?? meter?.message}
        </p>
      ) : null}
    </div>
  );
}

function getStatusLabel(
  enabled: boolean,
  meter: AudioSourceMeters["microphone"] | null,
  level: number,
) {
  if (!enabled) return "Off";
  if (!meter) return "Checking";
  if (meter.status === "degraded") return "Unavailable";
  if (meter.status === "unavailable") return "Unavailable";
  return level > 0.06 ? "Signal" : "Listening";
}

function formatStartupError(error: unknown) {
  return error instanceof Error ? error.message : "Could not start the live audio meter.";
}
