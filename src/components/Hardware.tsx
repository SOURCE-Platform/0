import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Separator } from "@/components/ui/separator";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

type HealthStatus = "online" | "degraded" | "offline";
type DeviceType = "sensor" | "server" | "wifi";
type HardwareTab = "sensors" | "server" | "connected";

interface Device {
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
  // percentage of viewBox (0-100)
  mapX?: number;
  mapY?: number;
}

// Floor plan coordinate system: viewBox="0 0 240 160"
// Rooms:
//   Bedroom 1:     x=0..60,   y=0..60
//   Bedroom 2:     x=60..120, y=0..60
//   Bedroom 3:     x=120..175,y=0..60
//   Bathroom 2:    x=175..240,y=0..30  (en-suite / Bed3 access)
//   Bathroom 1:    x=175..240,y=30..60 (main hallway bath)
//   Hallway:       x=0..210,  y=60..75
//   Server Closet: x=210..240,y=60..75
//   Living Room:   x=0..80,   y=75..160
//   Kitchen:       x=80..160, y=75..160
//   Den:           x=160..240,y=75..160
//
// Sensor positions (on outer walls):
//   North: (120, 0)  → mapX=50, mapY=0
//   South: (120, 160)→ mapX=50, mapY=100
//   East:  (240, 80) → mapX=100,mapY=50
//   West:  (0, 80)   → mapX=0,  mapY=50
//   Server closet center: (225, 67.5) → mapX=94, mapY=42

// Cable routes: each sensor's path follows wall center-lines to the server.
// Coordinates are in the SVG viewBox space (0 0 240 160).
// Server closet is at x=210..240, y=60..75. Server node is at (225, 67.5).
const CABLE_ROUTES: Record<string, [number, number][]> = {
  "sensor-north":          [[120,0],    [175,0],    [175,60],   [210,60],   [210,67.5], [225,67.5]],
  "sensor-south":          [[120,160],  [160,160],  [160,75],   [210,75],   [210,67.5], [225,67.5]],
  "sensor-east":           [[240,80],   [240,67.5], [225,67.5]],
  "sensor-west":           [[0,80],     [0,75],     [210,75],   [210,67.5], [225,67.5]],
  "sensor-bed1-bed2":      [[60,30],    [60,60],    [210,60],   [210,67.5], [225,67.5]],
  "sensor-bed2-bed3":      [[120,30],   [120,60],   [210,60],   [210,67.5], [225,67.5]],
  "sensor-bed3-bath":      [[175,30],   [175,60],   [210,60],   [210,67.5], [225,67.5]],
  "sensor-bath-div":       [[207.5,30], [207.5,60], [210,60],   [210,67.5], [225,67.5]],
  "sensor-bed1-south":     [[30,60],    [210,60],   [210,67.5], [225,67.5]],
  "sensor-bed2-south":     [[90,60],    [210,60],   [210,67.5], [225,67.5]],
  "sensor-bed3-south":     [[148,60],   [175,60],   [210,60],   [210,67.5], [225,67.5]],
  "sensor-hallway-south":  [[130,75],   [210,75],   [210,67.5], [225,67.5]],
  "sensor-living-kitchen": [[80,96.5],  [80,75],    [210,75],   [210,67.5], [225,67.5]],
  "sensor-kitchen-den":    [[160,96.5], [160,75],   [210,75],   [210,67.5], [225,67.5]],
};

const INITIAL_DEVICES: Device[] = [
  // ── Outer perimeter sensors ──
  {
    id: "sensor-north",
    name: "North Wall Sensor",
    type: "sensor",
    status: "online",
    location: "North Wall",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.11",
    mac: "A4:CF:12:77:01:01",
    firmware: "2.4.1",
    enabled: true,
    mapX: 50,       // SVG x=120
    mapY: 0,        // SVG y=0  (top outer wall center)
  },
  {
    id: "sensor-south",
    name: "South Wall Sensor",
    type: "sensor",
    status: "online",
    location: "South Wall",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.12",
    mac: "A4:CF:12:77:01:02",
    firmware: "2.4.1",
    enabled: true,
    mapX: 50,       // SVG x=120
    mapY: 100,      // SVG y=160 (bottom outer wall center)
  },
  {
    id: "sensor-east",
    name: "East Wall Sensor",
    type: "sensor",
    status: "degraded",
    location: "East Wall",
    installDate: "2024-03-15",
    isReplacement: true,
    replacedDate: "2025-01-08",
    ip: "192.168.1.13",
    mac: "A4:CF:12:77:01:03",
    firmware: "2.3.9",
    enabled: true,
    mapX: 100,      // SVG x=240 (right outer wall center)
    mapY: 50,       // SVG y=80
  },
  {
    id: "sensor-west",
    name: "West Wall Sensor",
    type: "sensor",
    status: "online",
    location: "West Wall",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.14",
    mac: "A4:CF:12:77:01:04",
    firmware: "2.4.1",
    enabled: true,
    mapX: 0,        // SVG x=0  (left outer wall center)
    mapY: 50,       // SVG y=80
  },
  // ── Interior wall sensors ──
  {
    id: "sensor-bed1-bed2",
    name: "Bedroom 1 / 2 Wall",
    type: "sensor",
    status: "online",
    location: "Bedroom 1–2 Partition",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.15",
    mac: "A4:CF:12:77:01:05",
    firmware: "2.4.1",
    enabled: true,
    mapX: 25,       // SVG x=60
    mapY: 18.75,    // SVG y=30
  },
  {
    id: "sensor-bed2-bed3",
    name: "Bedroom 2 / 3 Wall",
    type: "sensor",
    status: "online",
    location: "Bedroom 2–3 Partition",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.16",
    mac: "A4:CF:12:77:01:06",
    firmware: "2.4.1",
    enabled: true,
    mapX: 50,       // SVG x=120
    mapY: 18.75,    // SVG y=30
  },
  {
    id: "sensor-bed3-bath",
    name: "Bedroom 3 / Bath Wall",
    type: "sensor",
    status: "online",
    location: "Bedroom 3–Bathroom Partition",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.17",
    mac: "A4:CF:12:77:01:07",
    firmware: "2.4.1",
    enabled: true,
    mapX: 72.9,     // SVG x=175
    mapY: 18.75,    // SVG y=30
  },
  {
    id: "sensor-bath-div",
    name: "Bathroom Divider",
    type: "sensor",
    status: "online",
    location: "Bath 1 / Bath 2 Divider",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.18",
    mac: "A4:CF:12:77:01:08",
    firmware: "2.4.1",
    enabled: true,
    mapX: 86.5,     // SVG x=207.5
    mapY: 18.75,    // SVG y=30
  },
  {
    id: "sensor-bed1-south",
    name: "Bedroom 1 South Wall",
    type: "sensor",
    status: "online",
    location: "Bedroom 1 / Hallway",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.19",
    mac: "A4:CF:12:77:01:09",
    firmware: "2.4.1",
    enabled: true,
    mapX: 12.5,     // SVG x=30
    mapY: 37.5,     // SVG y=60
  },
  {
    id: "sensor-bed2-south",
    name: "Bedroom 2 South Wall",
    type: "sensor",
    status: "online",
    location: "Bedroom 2 / Hallway",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.20",
    mac: "A4:CF:12:77:01:0A",
    firmware: "2.4.1",
    enabled: true,
    mapX: 37.5,     // SVG x=90
    mapY: 37.5,     // SVG y=60
  },
  {
    id: "sensor-bed3-south",
    name: "Bedroom 3 South Wall",
    type: "sensor",
    status: "online",
    location: "Bedroom 3 / Hallway",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.21",
    mac: "A4:CF:12:77:01:0B",
    firmware: "2.4.1",
    enabled: true,
    mapX: 61.7,     // SVG x=148
    mapY: 37.5,     // SVG y=60
  },
  {
    id: "sensor-hallway-south",
    name: "Hallway South Wall",
    type: "sensor",
    status: "online",
    location: "Hallway / Main Area",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.22",
    mac: "A4:CF:12:77:01:0C",
    firmware: "2.4.1",
    enabled: true,
    mapX: 54.2,     // SVG x=130
    mapY: 46.9,     // SVG y=75
  },
  {
    id: "sensor-living-kitchen",
    name: "Living Room / Kitchen Wall",
    type: "sensor",
    status: "online",
    location: "Living–Kitchen Partition",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.23",
    mac: "A4:CF:12:77:01:0D",
    firmware: "2.4.1",
    enabled: true,
    mapX: 33.3,     // SVG x=80
    mapY: 60.3,     // SVG y=96.5
  },
  {
    id: "sensor-kitchen-den",
    name: "Kitchen / Den Wall",
    type: "sensor",
    status: "online",
    location: "Kitchen–Den Partition",
    installDate: "2024-03-15",
    isReplacement: false,
    ip: "192.168.1.24",
    mac: "A4:CF:12:77:01:0E",
    firmware: "2.4.1",
    enabled: true,
    mapX: 66.7,     // SVG x=160
    mapY: 60.3,     // SVG y=96.5
  },
  {
    id: "server-main",
    name: "Central Server",
    type: "server",
    status: "online",
    location: "Server Closet",
    installDate: "2024-02-01",
    isReplacement: false,
    ip: "192.168.1.2",
    mac: "B8:27:EB:AA:11:22",
    firmware: "1.9.3",
    mapX: 94,
    mapY: 42,
  },
  {
    id: "wifi-laptop",
    name: "MacBook Pro",
    type: "wifi",
    status: "online",
    location: "Office",
    installDate: "2024-06-20",
    isReplacement: false,
    ip: "192.168.1.101",
    mac: "3C:22:FB:44:55:66",
    firmware: "macOS 15.3",
  },
  {
    id: "wifi-phone",
    name: "iPhone 15 Pro",
    type: "wifi",
    status: "online",
    location: "Mobile",
    installDate: "2024-09-10",
    isReplacement: false,
    ip: "192.168.1.102",
    mac: "1A:2B:3C:4D:5E:6F",
    firmware: "iOS 18.2",
  },
  {
    id: "wifi-tablet",
    name: "iPad Air",
    type: "wifi",
    status: "offline",
    location: "Living Room",
    installDate: "2023-11-04",
    isReplacement: false,
    ip: "192.168.1.103",
    mac: "AA:BB:CC:DD:EE:FF",
    firmware: "iPadOS 17.6",
  },
];

const STATUS_LABEL: Record<HealthStatus, string> = {
  online: "Online",
  degraded: "Degraded",
  offline: "Offline",
};

const TYPE_LABEL: Record<DeviceType, string> = {
  sensor: "Smart Sensor",
  server: "Server",
  wifi: "Wi-Fi Device",
};

function statusColor(s: HealthStatus) {
  return s === "online" ? "#10b981" : s === "degraded" ? "#fbbf24" : "#ef4444";
}

function statusRing(s: HealthStatus) {
  return s === "online"
    ? "ring-emerald-500/30"
    : s === "degraded"
    ? "ring-amber-400/30"
    : "ring-red-500/30";
}

function statusBg(s: HealthStatus) {
  return s === "online"
    ? "bg-emerald-500"
    : s === "degraded"
    ? "bg-amber-400"
    : "bg-red-500";
}

function statusText(s: HealthStatus) {
  return s === "online"
    ? "text-emerald-600 dark:text-emerald-400"
    : s === "degraded"
    ? "text-amber-600 dark:text-amber-400"
    : "text-red-600 dark:text-red-400";
}

function statusBorder(s: HealthStatus) {
  return s === "online"
    ? "border-emerald-500/40"
    : s === "degraded"
    ? "border-amber-400/40"
    : "border-red-500/40";
}

function HealthDot({ status }: { status: HealthStatus }) {
  return (
    <span
      className={`inline-block rounded-full ring-4 w-3 h-3 ${statusBg(status)} ${statusRing(status)} shrink-0`}
    />
  );
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export default function Hardware() {
  const [devices, setDevices] = useState<Device[]>(INITIAL_DEVICES);
  const [selectedId, setSelectedId] = useState<string | null>("sensor-north");
  const [activeTab, setActiveTab] = useState<HardwareTab>("sensors");

  const selected = devices.find((d) => d.id === selectedId) ?? null;

  function toggleSensor(id: string, enabled: boolean) {
    setDevices((prev) => prev.map((d) => (d.id === id ? { ...d, enabled } : d)));
  }

  const sensors = devices.filter((d) => d.type === "sensor");
  const servers = devices.filter((d) => d.type === "server");
  const wifiDevices = devices.filter((d) => d.type === "wifi");

  return (
    <div className="w-full max-w-6xl mx-auto space-y-6">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">Hardware</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Manage smart sensors, servers, and connected devices on your network.
        </p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-[1fr_340px] gap-6 items-start">
        {/* Left: Tabs + content */}
        <div className="rounded-lg border bg-card overflow-hidden">
          <div className="px-4 pt-4 pb-0">
            <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as HardwareTab)}>
              <TabsList variant="line">
                <TabsTrigger value="sensors">Sensors</TabsTrigger>
                <TabsTrigger value="server">Server</TabsTrigger>
                <TabsTrigger value="connected">Connected Devices</TabsTrigger>
              </TabsList>
            </Tabs>
          </div>

          <Separator />

          {activeTab === "sensors" && (
            <div>
              <div className="p-4">
                <FloorPlanMap
                  devices={devices}
                  selectedId={selectedId}
                  onSelect={setSelectedId}
                />
              </div>
              <Separator />
              <div className="divide-y divide-border/50">
                {sensors.map((d) => (
                  <DeviceRow
                    key={d.id}
                    device={d}
                    isSelected={d.id === selectedId}
                    onSelect={() => setSelectedId(d.id)}
                    onToggle={toggleSensor}
                  />
                ))}
              </div>
            </div>
          )}

          {activeTab === "server" && (
            <div className="divide-y divide-border/50">
              {servers.map((d) => (
                <DeviceRow
                  key={d.id}
                  device={d}
                  isSelected={d.id === selectedId}
                  onSelect={() => setSelectedId(d.id)}
                />
              ))}
            </div>
          )}

          {activeTab === "connected" && (
            <div className="divide-y divide-border/50">
              {wifiDevices.map((d) => (
                <DeviceRow
                  key={d.id}
                  device={d}
                  isSelected={d.id === selectedId}
                  onSelect={() => setSelectedId(d.id)}
                />
              ))}
            </div>
          )}
        </div>

        {/* Right: Detail panel — always visible */}
        <div className="rounded-lg border bg-card sticky top-6">
          {selected ? (
            <DeviceDetail device={selected} onToggle={toggleSensor} />
          ) : (
            <div className="p-8 text-center text-muted-foreground text-sm">
              Select a device to view details
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Architectural Floor Plan ─────────────────────────────────────────────────

function FloorPlanMap({
  devices,
  selectedId,
  onSelect,
}: {
  devices: Device[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  const VW = 240;
  const VH = 160;

  const mapped = devices.filter((d) => d.mapX !== undefined && d.mapY !== undefined);
  const sensors = mapped.filter((d) => d.type === "sensor");

  function px(pct: number, total: number) {
    return (pct / 100) * total;
  }

  return (
    <div className="w-full bg-muted/10 rounded-md border border-border/30 overflow-hidden">
      <svg
        viewBox={`0 0 ${VW} ${VH}`}
        className="w-full h-auto"
        style={{ display: "block" }}
      >
        {/* ── Room fills ── */}
        {/* Bedrooms */}
        <rect x="0"   y="0" width="60"  height="60" className="fill-card/50" />
        <rect x="60"  y="0" width="60"  height="60" className="fill-card/50" />
        <rect x="120" y="0" width="55"  height="60" className="fill-card/50" />
        {/* Bathrooms */}
        <rect x="175" y="0"  width="65" height="30" className="fill-muted/40" />
        <rect x="175" y="30" width="65" height="30" className="fill-muted/40" />
        {/* Hallway */}
        <rect x="0"   y="60" width="210" height="15" className="fill-muted/20" />
        {/* Server closet */}
        <rect x="210" y="60" width="30"  height="15" className="fill-muted/50" />
        {/* Living / Kitchen / Den */}
        <rect x="0"   y="75" width="80"  height="85" className="fill-card/50" />
        <rect x="80"  y="75" width="80"  height="85" className="fill-card/40" />
        <rect x="160" y="75" width="80"  height="85" className="fill-card/50" />

        {/* ── Ethernet cables — routed through wall center-lines ── */}
        {sensors.map((s) => {
          const route = CABLE_ROUTES[s.id];
          if (!route) return null;
          const pts = route.map(([x, y]) => `${x},${y}`).join(" ");
          return (
            <polyline
              key={s.id}
              points={pts}
              stroke="currentColor"
              strokeWidth="0.6"
              strokeDasharray="2.5 1.8"
              fill="none"
              className="text-primary/30"
            />
          );
        })}

        {/* ── Outer perimeter walls ── */}
        <rect
          x="0" y="0" width="240" height="160"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.5"
          className="text-foreground/80"
        />

        {/* ── Interior walls ── */}
        <g
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="square"
          fill="none"
          className="text-foreground/70"
        >
          {/* Bedroom vertical dividers */}
          <line x1="60"  y1="0" x2="60"  y2="60" />
          <line x1="120" y1="0" x2="120" y2="60" />

          {/* Bed3 / Baths vertical wall — gap y=10..22 for en-suite door */}
          <line x1="175" y1="0"  x2="175" y2="10" />
          <line x1="175" y1="22" x2="175" y2="60" />

          {/* Bath1 / Bath2 horizontal divider */}
          <line x1="175" y1="30" x2="240" y2="30" />

          {/* Wall below bedrooms (y=60) — door gaps:
              Bed1: x=20..32   Bed2: x=80..92   Bed3: x=140..152
              Bath1 (main): x=195..205 */}
          <line x1="0"   y1="60" x2="20"  y2="60" />
          <line x1="32"  y1="60" x2="80"  y2="60" />
          <line x1="92"  y1="60" x2="140" y2="60" />
          <line x1="152" y1="60" x2="195" y2="60" />
          <line x1="205" y1="60" x2="240" y2="60" />

          {/* Server closet partition in hallway */}
          <line x1="210" y1="60" x2="210" y2="75" />

          {/* Wall below hallway (y=75) — archway to living room x=12..56 */}
          <line x1="0"   y1="75" x2="12"  y2="75" />
          <line x1="56"  y1="75" x2="240" y2="75" />

          {/* Kitchen partial counter walls (open plan) */}
          <line x1="80"  y1="75" x2="80"  y2="118" />
          <line x1="160" y1="75" x2="160" y2="118" />
        </g>

        {/* ── Kitchen island ── */}
        <rect
          x="98" y="105" width="24" height="10"
          fill="none"
          stroke="currentColor"
          strokeWidth="0.8"
          className="text-foreground/40"
          rx="0.5"
        />

        {/* ── Door indicators ── */}
        <g
          stroke="currentColor"
          strokeWidth="0.7"
          fill="none"
          className="text-muted-foreground/55"
        >
          {/* Bedroom 1 → Hallway: hinge x=20,y=60, opens down */}
          <line x1="20" y1="60" x2="20" y2="72" />
          <path d="M 20,72 A 12,12 0 0 0 32,60" />

          {/* Bedroom 2 → Hallway: hinge x=80,y=60, opens down */}
          <line x1="80" y1="60" x2="80" y2="72" />
          <path d="M 80,72 A 12,12 0 0 0 92,60" />

          {/* Bedroom 3 → Hallway: hinge x=140,y=60, opens down */}
          <line x1="140" y1="60" x2="140" y2="72" />
          <path d="M 140,72 A 12,12 0 0 0 152,60" />

          {/* Bathroom 1 (main) → Hallway: hinge x=205,y=60, opens up into bath */}
          <line x1="205" y1="60" x2="205" y2="50" />
          <path d="M 205,50 A 10,10 0 0 0 195,60" />

          {/* Bathroom 2 en-suite → Bedroom 3: hinge (175,10), opens right */}
          <line x1="175" y1="10" x2="187" y2="10" />
          <path d="M 187,10 A 12,12 0 0 1 175,22" />

          {/* Hallway → Living Room archway markers (small corner ticks) */}
          <line x1="12" y1="72" x2="12" y2="75" />
          <line x1="56" y1="72" x2="56" y2="75" />
        </g>

        {/* ── Room labels ── */}
        <g
          fill="currentColor"
          className="text-muted-foreground/45"
          fontFamily="system-ui, sans-serif"
          fontSize="4.5"
          textAnchor="middle"
        >
          <text x="30"  y="33">Bedroom 1</text>
          <text x="90"  y="33">Bedroom 2</text>
          <text x="148" y="33">Bedroom 3</text>
          <text x="207" y="17">Bath 2</text>
          <text x="207" y="48">Bath 1</text>
          <text x="100" y="70">Hallway</text>
          <text x="228" y="70" fontSize="3.5">Server</text>
          <text x="40"  y="120">Living Room</text>
          <text x="120" y="120">Kitchen</text>
          <text x="200" y="120">Den</text>
        </g>

        {/* ── Compass rose (small, top-left) ── */}
        <g fill="currentColor" className="text-muted-foreground/30" fontSize="3.5" textAnchor="middle" fontFamily="system-ui">
          <text x="8" y="5">N</text>
          <text x="8" y="158">S</text>
          <text x="235" y="82">E</text>
          <text x="5" y="82">W</text>
        </g>

        {/* ── Device nodes ── */}
        {mapped.map((device) => {
          const cx = px(device.mapX!, VW);
          const cy = px(device.mapY!, VH);
          const isSelected = device.id === selectedId;
          const col = statusColor(device.status);

          return (
            <g
              key={device.id}
              onClick={() => onSelect(device.id)}
              className="cursor-pointer"
            >
              {/* Selection ring */}
              {isSelected && (
                <circle cx={cx} cy={cy} r="5.5" fill={col} fillOpacity="0.25" />
              )}
              {/* Node dot */}
              <circle
                cx={cx}
                cy={cy}
                r="3"
                fill={col}
                stroke="white"
                strokeWidth="0.8"
                strokeOpacity="0.9"
              />
              {/* Server gets a square instead */}
              {device.type === "server" && (
                <rect
                  x={cx - 3} y={cy - 3}
                  width="6" height="6"
                  fill={col}
                  stroke="white"
                  strokeWidth="0.8"
                  strokeOpacity="0.9"
                  rx="0.5"
                />
              )}
            </g>
          );
        })}
      </svg>
    </div>
  );
}

// ─── Device Row ───────────────────────────────────────────────────────────────

function DeviceRow({
  device,
  isSelected,
  onSelect,
  onToggle,
}: {
  device: Device;
  isSelected: boolean;
  onSelect: () => void;
  onToggle?: (id: string, enabled: boolean) => void;
}) {
  return (
    <div
      className={`flex items-center gap-3 px-4 py-3 cursor-pointer transition-colors hover:bg-muted/40 ${
        isSelected ? "bg-muted/60" : ""
      }`}
      onClick={onSelect}
    >
      <HealthDot status={device.status} />
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium truncate">{device.name}</p>
        <p className="text-xs text-muted-foreground truncate">{device.location}</p>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        <Badge
          variant="outline"
          className={`text-xs px-2 py-0 h-5 ${statusBorder(device.status)} ${statusText(device.status)}`}
        >
          {STATUS_LABEL[device.status]}
        </Badge>
        {device.type === "sensor" && onToggle && (
          <Switch
            checked={device.enabled ?? true}
            onCheckedChange={(checked) => onToggle(device.id, checked)}
            onClick={(e) => e.stopPropagation()}
            className="scale-[0.8]"
            aria-label={`Toggle ${device.name}`}
          />
        )}
      </div>
    </div>
  );
}

// ─── Device Detail Panel ──────────────────────────────────────────────────────

function DeviceDetail({
  device,
  onToggle,
}: {
  device: Device;
  onToggle: (id: string, enabled: boolean) => void;
}) {
  return (
    <ScrollArea className="max-h-[600px]">
      <div className="p-5 space-y-5">
        <div className="flex items-start gap-3">
          <HealthDot status={device.status} />
          <div className="flex-1 min-w-0">
            <h3 className="font-semibold text-base leading-tight">{device.name}</h3>
            <p className="text-xs text-muted-foreground mt-0.5">{TYPE_LABEL[device.type]}</p>
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
          {device.type === "sensor" && (
            <DetailRow label="Sensor Active">
              <Switch
                checked={device.enabled ?? true}
                onCheckedChange={(checked) => onToggle(device.id, checked)}
                aria-label={`Toggle ${device.name}`}
              />
            </DetailRow>
          )}
        </div>

        <Separator />

        <div className="space-y-3">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
            Installation
          </p>
          <DetailRow label="Install Date">
            <span className="text-sm">{formatDate(device.installDate)}</span>
          </DetailRow>
          <DetailRow label="Unit">
            {device.isReplacement ? (
              <div className="text-right">
                <Badge variant="secondary" className="text-xs">Replacement</Badge>
                {device.replacedDate && (
                  <p className="text-xs text-muted-foreground mt-1">
                    Replaced {formatDate(device.replacedDate)}
                  </p>
                )}
              </div>
            ) : (
              <Badge variant="outline" className="text-xs">Original</Badge>
            )}
          </DetailRow>
        </div>

        {(device.ip || device.mac) && (
          <>
            <Separator />
            <div className="space-y-3">
              <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
                Network
              </p>
              {device.ip && (
                <DetailRow label="IP Address">
                  <span className="text-sm font-mono">{device.ip}</span>
                </DetailRow>
              )}
              {device.mac && (
                <DetailRow label="MAC Address">
                  <span className="text-sm font-mono text-xs">{device.mac}</span>
                </DetailRow>
              )}
              <DetailRow label="Connection">
                <span className="text-sm text-muted-foreground">
                  {device.type === "wifi" ? "Wi-Fi" : "Cat 6 Shielded"}
                </span>
              </DetailRow>
            </div>
          </>
        )}

        {device.firmware && (
          <>
            <Separator />
            <div className="space-y-3">
              <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
                Software
              </p>
              <DetailRow label="Firmware / OS">
                <span className="text-sm font-mono">{device.firmware}</span>
              </DetailRow>
            </div>
          </>
        )}
      </div>
    </ScrollArea>
  );
}

function DetailRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <span className="text-sm text-muted-foreground shrink-0">{label}</span>
      <div className="text-right">{children}</div>
    </div>
  );
}
