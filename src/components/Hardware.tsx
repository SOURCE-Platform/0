import { useState } from "react";
import { Separator } from "@/components/ui/separator";
import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { DeviceDetail } from "@/components/hardware/DeviceDetail";
import { DeviceRow } from "@/components/hardware/DeviceRow";
import { INITIAL_DEVICES } from "@/components/hardware/data";
import { FloorPlanMap } from "@/components/hardware/FloorPlanMap";
import { Device, HardwareTab } from "@/components/hardware/types";

export default function Hardware() {
  const [devices, setDevices] = useState<Device[]>(INITIAL_DEVICES);
  const [selectedId, setSelectedId] = useState<string | null>("sensor-north");
  const [activeTab, setActiveTab] = useState<HardwareTab>("sensors");

  const selected = devices.find((device) => device.id === selectedId) ?? null;
  const sensors = devices.filter((device) => device.type === "sensor");
  const servers = devices.filter((device) => device.type === "server");
  const wifiDevices = devices.filter((device) => device.type === "wifi");

  function toggleSensor(id: string, enabled: boolean) {
    setDevices((previous) =>
      previous.map((device) => (device.id === id ? { ...device, enabled } : device)),
    );
  }

  const tabItems = [
    { value: "sensors", label: "Sensors" },
    { value: "server", label: "Server" },
    { value: "connected", label: "Connected Devices" },
  ] as const;

  return (
    <div className="mx-auto w-full max-w-6xl space-y-6">
      <div>
        <h1 className="text-xl font-medium tracking-tight">Hardware</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Manage smart sensors, servers, and connected devices on your network.
        </p>
      </div>

      <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-[1fr_340px]">
        <div className="overflow-hidden rounded-lg border bg-card">
          <div className="px-4 pb-0 pt-4">
            <AnimatedTabNav
              tabs={tabItems.map((tab) => ({ value: tab.value, label: tab.label }))}
              value={activeTab}
              onValueChange={(value) => setActiveTab(value as HardwareTab)}
            />
          </div>

          <Separator />

          {activeTab === "sensors" ? (
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
                {sensors.map((device) => (
                  <DeviceRow
                    key={device.id}
                    device={device}
                    isSelected={device.id === selectedId}
                    onSelect={() => setSelectedId(device.id)}
                    onToggle={toggleSensor}
                  />
                ))}
              </div>
            </div>
          ) : null}

          {activeTab === "server" ? (
            <div className="divide-y divide-border/50">
              {servers.map((device) => (
                <DeviceRow
                  key={device.id}
                  device={device}
                  isSelected={device.id === selectedId}
                  onSelect={() => setSelectedId(device.id)}
                />
              ))}
            </div>
          ) : null}

          {activeTab === "connected" ? (
            <div className="divide-y divide-border/50">
              {wifiDevices.map((device) => (
                <DeviceRow
                  key={device.id}
                  device={device}
                  isSelected={device.id === selectedId}
                  onSelect={() => setSelectedId(device.id)}
                />
              ))}
            </div>
          ) : null}
        </div>

        <div className="sticky top-6 rounded-lg border bg-card">
          {selected ? (
            <DeviceDetail device={selected} onToggle={toggleSensor} />
          ) : (
            <div className="p-8 text-center text-sm text-muted-foreground">
              Select a device to view details
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
