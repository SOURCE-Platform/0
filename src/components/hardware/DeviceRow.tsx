import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { HealthDot, STATUS_LABEL, statusBorder, statusText } from "@/components/hardware/status";
import { Device } from "@/components/hardware/types";

interface DeviceRowProps {
  device: Device;
  isSelected: boolean;
  onSelect: () => void;
  onToggle?: (id: string, enabled: boolean) => void;
}

export function DeviceRow({ device, isSelected, onSelect, onToggle }: DeviceRowProps) {
  return (
    <div
      className={`flex cursor-pointer items-center gap-3 px-4 py-3 transition-colors hover:bg-muted/40 ${
        isSelected ? "bg-muted/60" : ""
      }`}
      onClick={onSelect}
    >
      <HealthDot status={device.status} />
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium">{device.name}</p>
        <p className="truncate text-xs text-muted-foreground">{device.location}</p>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <Badge
          variant="outline"
          className={`h-5 px-2 py-0 text-xs ${statusBorder(device.status)} ${statusText(device.status)}`}
        >
          {STATUS_LABEL[device.status]}
        </Badge>
        {device.type === "sensor" && onToggle ? (
          <Switch
            checked={device.enabled ?? true}
            onCheckedChange={(checked) => onToggle(device.id, checked)}
            onClick={(event) => event.stopPropagation()}
            className="scale-[0.8]"
            aria-label={`Toggle ${device.name}`}
          />
        ) : null}
      </div>
    </div>
  );
}
