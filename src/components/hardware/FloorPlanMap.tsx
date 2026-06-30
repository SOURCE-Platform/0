import { CABLE_ROUTES } from "@/components/hardware/data";
import { statusColor } from "@/components/hardware/status";
import { Device } from "@/components/hardware/types";

interface FloorPlanMapProps {
  devices: Device[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

const VIEWBOX_WIDTH = 240;
const VIEWBOX_HEIGHT = 160;

export function FloorPlanMap({ devices, selectedId, onSelect }: FloorPlanMapProps) {
  const mappedDevices = devices.filter((device) => device.mapX !== undefined && device.mapY !== undefined);
  const sensorDevices = mappedDevices.filter((device) => device.type === "sensor");

  function percentToPx(percent: number, total: number) {
    return (percent / 100) * total;
  }

  return (
    <div className="w-full overflow-hidden rounded-md border border-border/30 bg-muted/10">
      <svg
        viewBox={`0 0 ${VIEWBOX_WIDTH} ${VIEWBOX_HEIGHT}`}
        className="h-auto w-full"
        style={{ display: "block" }}
      >
        <FloorPlanRooms />
        <FloorPlanCables sensors={sensorDevices} />
        <FloorPlanWalls />
        <FloorPlanDoors />
        <FloorPlanLabels />

        {mappedDevices.map((device) => {
          const cx = percentToPx(device.mapX!, VIEWBOX_WIDTH);
          const cy = percentToPx(device.mapY!, VIEWBOX_HEIGHT);
          const isSelected = device.id === selectedId;
          const color = statusColor(device.status);

          return (
            <g key={device.id} onClick={() => onSelect(device.id)} className="cursor-pointer">
              {isSelected ? <circle cx={cx} cy={cy} r="5.5" fill={color} fillOpacity="0.25" /> : null}
              <circle
                cx={cx}
                cy={cy}
                r="3"
                fill={color}
                stroke="white"
                strokeWidth="0.8"
                strokeOpacity="0.9"
              />
              {device.type === "server" ? (
                <rect
                  x={cx - 3}
                  y={cy - 3}
                  width="6"
                  height="6"
                  fill={color}
                  stroke="white"
                  strokeWidth="0.8"
                  strokeOpacity="0.9"
                  rx="0.5"
                />
              ) : null}
            </g>
          );
        })}
      </svg>
    </div>
  );
}

function FloorPlanRooms() {
  return (
    <>
      <rect x="0" y="0" width="60" height="60" className="fill-card/50" />
      <rect x="60" y="0" width="60" height="60" className="fill-card/50" />
      <rect x="120" y="0" width="55" height="60" className="fill-card/50" />
      <rect x="175" y="0" width="65" height="30" className="fill-muted/40" />
      <rect x="175" y="30" width="65" height="30" className="fill-muted/40" />
      <rect x="0" y="60" width="210" height="15" className="fill-muted/20" />
      <rect x="210" y="60" width="30" height="15" className="fill-muted/50" />
      <rect x="0" y="75" width="80" height="85" className="fill-card/50" />
      <rect x="80" y="75" width="80" height="85" className="fill-card/40" />
      <rect x="160" y="75" width="80" height="85" className="fill-card/50" />
      <rect
        x="98"
        y="105"
        width="24"
        height="10"
        fill="none"
        stroke="currentColor"
        strokeWidth="0.8"
        className="text-foreground/40"
        rx="0.5"
      />
    </>
  );
}

function FloorPlanCables({ sensors }: { sensors: Device[] }) {
  return (
    <>
      {sensors.map((sensor) => {
        const route = CABLE_ROUTES[sensor.id];
        if (!route) return null;
        const points = route.map(([x, y]) => `${x},${y}`).join(" ");
        return (
          <polyline
            key={sensor.id}
            points={points}
            stroke="currentColor"
            strokeWidth="0.6"
            strokeDasharray="2.5 1.8"
            fill="none"
            className="text-primary/30"
          />
        );
      })}
    </>
  );
}

function FloorPlanWalls() {
  return (
    <>
      <rect
        x="0"
        y="0"
        width="240"
        height="160"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        className="text-foreground/80"
      />
      <g
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="square"
        fill="none"
        className="text-foreground/70"
      >
        <line x1="60" y1="0" x2="60" y2="60" />
        <line x1="120" y1="0" x2="120" y2="60" />
        <line x1="175" y1="0" x2="175" y2="10" />
        <line x1="175" y1="22" x2="175" y2="60" />
        <line x1="175" y1="30" x2="240" y2="30" />
        <line x1="0" y1="60" x2="20" y2="60" />
        <line x1="32" y1="60" x2="80" y2="60" />
        <line x1="92" y1="60" x2="140" y2="60" />
        <line x1="152" y1="60" x2="195" y2="60" />
        <line x1="205" y1="60" x2="240" y2="60" />
        <line x1="210" y1="60" x2="210" y2="75" />
        <line x1="0" y1="75" x2="12" y2="75" />
        <line x1="56" y1="75" x2="240" y2="75" />
        <line x1="80" y1="75" x2="80" y2="118" />
        <line x1="160" y1="75" x2="160" y2="118" />
      </g>
    </>
  );
}

function FloorPlanDoors() {
  return (
    <g
      stroke="currentColor"
      strokeWidth="0.7"
      fill="none"
      className="text-muted-foreground/55"
    >
      <line x1="20" y1="60" x2="20" y2="72" />
      <path d="M 20,72 A 12,12 0 0 0 32,60" />
      <line x1="80" y1="60" x2="80" y2="72" />
      <path d="M 80,72 A 12,12 0 0 0 92,60" />
      <line x1="140" y1="60" x2="140" y2="72" />
      <path d="M 140,72 A 12,12 0 0 0 152,60" />
      <line x1="205" y1="60" x2="205" y2="50" />
      <path d="M 205,50 A 10,10 0 0 0 195,60" />
      <line x1="175" y1="10" x2="187" y2="10" />
      <path d="M 187,10 A 12,12 0 0 1 175,22" />
      <line x1="12" y1="72" x2="12" y2="75" />
      <line x1="56" y1="72" x2="56" y2="75" />
    </g>
  );
}

function FloorPlanLabels() {
  return (
    <>
      <g
        fill="currentColor"
        className="text-muted-foreground/45"
        fontFamily="system-ui, sans-serif"
        fontSize="4.5"
        textAnchor="middle"
      >
        <text x="30" y="33">
          Bedroom 1
        </text>
        <text x="90" y="33">
          Bedroom 2
        </text>
        <text x="148" y="33">
          Bedroom 3
        </text>
        <text x="207" y="17">
          Bath 2
        </text>
        <text x="207" y="48">
          Bath 1
        </text>
        <text x="100" y="70">
          Hallway
        </text>
        <text x="228" y="70" fontSize="3.5">
          Server
        </text>
        <text x="40" y="120">
          Living Room
        </text>
        <text x="120" y="120">
          Kitchen
        </text>
        <text x="200" y="120">
          Den
        </text>
      </g>
      <g
        fill="currentColor"
        className="text-muted-foreground/30"
        fontSize="3.5"
        textAnchor="middle"
        fontFamily="system-ui"
      >
        <text x="8" y="5">
          N
        </text>
        <text x="8" y="158">
          S
        </text>
        <text x="235" y="82">
          E
        </text>
        <text x="5" y="82">
          W
        </text>
      </g>
    </>
  );
}
