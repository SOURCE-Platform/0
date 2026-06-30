import { HealthStatus, DeviceType } from "@/components/hardware/types";

export const STATUS_LABEL: Record<HealthStatus, string> = {
  online: "Online",
  degraded: "Degraded",
  offline: "Offline",
};

export const TYPE_LABEL: Record<DeviceType, string> = {
  sensor: "Smart Sensor",
  server: "Server",
  wifi: "Wi-Fi Device",
};

export function statusColor(status: HealthStatus) {
  return status === "online" ? "#10b981" : status === "degraded" ? "#fbbf24" : "#ef4444";
}

export function statusRing(status: HealthStatus) {
  return status === "online"
    ? "ring-emerald-500/30"
    : status === "degraded"
      ? "ring-amber-400/30"
      : "ring-red-500/30";
}

export function statusBg(status: HealthStatus) {
  return status === "online"
    ? "bg-emerald-500"
    : status === "degraded"
      ? "bg-amber-400"
      : "bg-red-500";
}

export function statusText(status: HealthStatus) {
  return status === "online"
    ? "text-emerald-600 dark:text-emerald-400"
    : status === "degraded"
      ? "text-amber-600 dark:text-amber-400"
      : "text-red-600 dark:text-red-400";
}

export function statusBorder(status: HealthStatus) {
  return status === "online"
    ? "border-emerald-500/40"
    : status === "degraded"
      ? "border-amber-400/40"
      : "border-red-500/40";
}

export function formatDate(value: string) {
  return new Date(value).toLocaleDateString("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export function HealthDot({ status }: { status: HealthStatus }) {
  return (
    <span
      className={`inline-block h-3 w-3 shrink-0 rounded-full ring-4 ${statusBg(status)} ${statusRing(status)}`}
    />
  );
}
