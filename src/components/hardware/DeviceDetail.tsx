import type { ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import {
  formatDate,
  HealthDot,
  STATUS_LABEL,
  statusText,
  TYPE_LABEL,
} from "@/components/hardware/status";
import { Device } from "@/components/hardware/types";

interface DeviceDetailProps {
  device: Device;
  onToggle: (id: string, enabled: boolean) => void;
}

export function DeviceDetail({ device, onToggle }: DeviceDetailProps) {
  return (
    <ScrollArea className="max-h-[600px]">
      <div className="space-y-5 p-5">
        <div className="flex items-start gap-3">
          <HealthDot status={device.status} />
          <div className="min-w-0 flex-1">
            <h3 className="text-base font-medium leading-tight">{device.name}</h3>
            <p className="mt-0.5 text-xs text-muted-foreground">{TYPE_LABEL[device.type]}</p>
          </div>
        </div>

        <Separator />

        <div className="space-y-3">
          <DetailRow label="Status">
            <span className={`text-sm font-medium ${statusText(device.status)}`}>
              {STATUS_LABEL[device.status]}
            </span>
          </DetailRow>
          <DetailRow label="Location">
            <span className="text-sm">{device.location}</span>
          </DetailRow>
          {device.type === "sensor" ? (
            <DetailRow label="Sensor Active">
              <Switch
                checked={device.enabled ?? true}
                onCheckedChange={(checked) => onToggle(device.id, checked)}
                aria-label={`Toggle ${device.name}`}
              />
            </DetailRow>
          ) : null}
        </div>

        <Separator />

        <SectionLabel>Installation</SectionLabel>
        <div className="space-y-3">
          <DetailRow label="Install Date">
            <span className="text-sm">{formatDate(device.installDate)}</span>
          </DetailRow>
          <DetailRow label="Unit">
            {device.isReplacement ? (
              <div className="text-right">
                <Badge variant="secondary" className="text-xs">
                  Replacement
                </Badge>
                {device.replacedDate ? (
                  <p className="mt-1 text-xs text-muted-foreground">
                    Replaced {formatDate(device.replacedDate)}
                  </p>
                ) : null}
              </div>
            ) : (
              <Badge variant="outline" className="text-xs">
                Original
              </Badge>
            )}
          </DetailRow>
        </div>

        {device.ip || device.mac ? (
          <>
            <Separator />
            <SectionLabel>Network</SectionLabel>
            <div className="space-y-3">
              {device.ip ? (
                <DetailRow label="IP Address">
                  <span className="font-mono text-sm">{device.ip}</span>
                </DetailRow>
              ) : null}
              {device.mac ? (
                <DetailRow label="MAC Address">
                  <span className="font-mono text-xs">{device.mac}</span>
                </DetailRow>
              ) : null}
              <DetailRow label="Connection">
                <span className="text-sm text-muted-foreground">
                  {device.type === "wifi" ? "Wi-Fi" : "Cat 6 Shielded"}
                </span>
              </DetailRow>
            </div>
          </>
        ) : null}

        {device.firmware ? (
          <>
            <Separator />
            <SectionLabel>Software</SectionLabel>
            <div className="space-y-3">
              <DetailRow label="Firmware / OS">
                <span className="font-mono text-sm">{device.firmware}</span>
              </DetailRow>
            </div>
          </>
        ) : null}
      </div>
    </ScrollArea>
  );
}

function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
      {children}
    </p>
  );
}

function DetailRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-4">
      <span className="shrink-0 text-sm text-muted-foreground">{label}</span>
      <div className="text-right">{children}</div>
    </div>
  );
}
