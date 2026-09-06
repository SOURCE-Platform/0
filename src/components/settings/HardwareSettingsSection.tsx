import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Label } from "@/components/ui/label";

interface HostHardwareInfo {
  model: string;
  chip: string;
  cpuCount: string;
  memory: string;
  os: string;
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-4 py-1.5">
      <Label className="text-sm text-muted-foreground">{label}</Label>
      <span className="text-sm font-medium">{value}</span>
    </div>
  );
}

export function HardwareSettingsSection() {
  const [info, setInfo] = useState<HostHardwareInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<HostHardwareInfo>("get_host_hardware_info")
      .then(setInfo)
      .catch((failure) => setError(String(failure)));
  }, []);

  return (
    <section className="rounded-lg border border-border/60 bg-muted/15 p-3">
      <Label className="text-sm">This Mac</Label>
      <p className="mt-1 max-w-[52ch] text-xs leading-5 text-muted-foreground">
        The computer SOURCE is running on. Home hardware inventory
        will live here later.
      </p>
      <div className="mt-3 divide-y divide-border/50">
        {error ? (
          <p className="py-2 text-sm text-destructive">{error}</p>
        ) : !info ? (
          <p className="py-2 text-sm text-muted-foreground">Loading hardware info…</p>
        ) : (
          <>
            <InfoRow label="Model" value={info.model} />
            <InfoRow label="Chip" value={info.chip} />
            <InfoRow label="CPUs" value={info.cpuCount} />
            <InfoRow label="Memory" value={info.memory} />
            <InfoRow label="OS" value={info.os} />
          </>
        )}
      </div>
    </section>
  );
}
