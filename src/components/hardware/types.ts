export type HealthStatus = "online" | "degraded" | "offline";
export type DeviceType = "sensor" | "server" | "wifi";
export type HardwareTab = "sensors" | "server" | "connected";

export interface Device {
  id: string;
  name: string;
  type: DeviceType;
  status: HealthStatus;
  location: string;
  installDate: string;
  isReplacement: boolean;
  replacedDate?: string;
  ip?: string;
  mac?: string;
  firmware?: string;
  enabled?: boolean;
  mapX?: number;
  mapY?: number;
}
