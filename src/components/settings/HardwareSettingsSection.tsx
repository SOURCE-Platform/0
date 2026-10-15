import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SettingsPanel } from "@/components/ui/page";
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
    <SettingsPanel title="This Mac" description="The computer running SOURCE and processing your captures locally.">
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
    </SettingsPanel>
  );
}
