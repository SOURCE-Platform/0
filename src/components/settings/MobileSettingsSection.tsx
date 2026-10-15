import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { MobileConnectionDetails } from "./MobileConnectionDetails";
import { Disclosure } from "@/components/ui/page";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { SettingsController } from "@/components/settings/useSettingsController";
import { showSettingsToast } from "@/components/settings/utils";

interface PairRequest {
  requestId: string;
  deviceName: string;
  sas: string;
}

interface MobileDevice {
  deviceId: string;
  deviceName: string;
  lastSeenAt: number;
}

export function MobileSettingsSection({ controller }: { controller: SettingsController }) {
  const { config } = controller;
  const [requests, setRequests] = useState<PairRequest[]>([]);
  const [qr, setQr] = useState<string | null>(null);
  const [qrError, setQrError] = useState<string | null>(null);
  const [justPaired, setJustPaired] = useState<string | null>(null);
  const [fingerprint, setFingerprint] = useState<string>("");
  const [port, setPort] = useState<number | null>(null);
  const [devices, setDevices] = useState<MobileDevice[]>([]);
  const [phoneConnected, setPhoneConnected] = useState(false);

  useEffect(() => {
    invoke<string>("mobile_tls_fingerprint").then(setFingerprint).catch(() => {});
    invoke<number>("mobile_server_port").then(setPort).catch(() => {});
    refreshDevices();
    refreshRequests();
    const stops: Array<() => void> = [];
    listen<{ connected: boolean }>("mobile-status", (event) => {
      setPhoneConnected(event.payload.connected);
    }).then((stop) => stops.push(stop));
    listen<{ deviceName: string }>("mobile-paired", (event) => {
      setJustPaired(event.payload.deviceName);
      setQr(null);
      void invoke("mobile_clear_pairing_qr").catch(() => {});
      void refreshDevices();
    }).then((stop) => stops.push(stop));
    listen<PairRequest>("mobile-pair-request", (event) => {
      setRequests((current) =>
        current.some((item) => item.requestId === event.payload.requestId)
          ? current
          : [...current, event.payload],
      );
    }).then((stop) => stops.push(stop));
    // Catch requests raised while this tab was closed.
    const poll = setInterval(refreshRequests, 3000);
    return () => {
      stops.forEach((stop) => stop());
      clearInterval(poll);
      // Retire the on-screen secret when this tab goes away.
      void invoke("mobile_clear_pairing_qr").catch(() => {});
    };
  }, []);

  async function showQr() {
    setJustPaired(null);
    setQrError(null);
    try {
      setQr(await invoke<string>("mobile_pairing_qr"));
    } catch (error) {
      setQr(null);
      setQrError(String(error));
    }
  }

  async function refreshRequests() {
    try {
      setRequests(await invoke<PairRequest[]>("mobile_pending_pair_requests"));
    } catch {
      // Server not running yet.
    }
  }

  async function resolveRequest(requestId: string, approve: boolean) {
    try {
      await invoke(approve ? "mobile_approve_pair" : "mobile_deny_pair", { requestId });
    } finally {
      setRequests((current) => current.filter((item) => item.requestId !== requestId));
      if (approve) await refreshDevices();
    }
  }

  // Goes through its dedicated command (which applies the change live, not
  // just to the config file); the settings autosave then writes the same value.
  async function setPhonePrompts(enabled: boolean) {
    controller.updateConfig({ mobile_agent_prompts_enabled: enabled });
    try {
      await invoke("set_mobile_agent_prompts_enabled", { enabled });
    } catch (error) {
      controller.updateConfig({ mobile_agent_prompts_enabled: !enabled });
      showSettingsToast({ type: "error", text: `Couldn't change phone prompts: ${error}` });
    }
  }

  async function setKeepBypass(enabled: boolean) {
    controller.updateConfig({ agent_prompts_keep_bypass: enabled });
    try {
      await invoke("set_agent_prompts_keep_bypass", { enabled });
    } catch (error) {
      controller.updateConfig({ agent_prompts_keep_bypass: !enabled });
      showSettingsToast({ type: "error", text: `Couldn't change Bypass permissions: ${error}` });
    }
  }

  async function refreshDevices() {
    try {
      const rows = await invoke<MobileDevice[]>("mobile_list_devices");
      setDevices(rows);
    } catch {
      // Server not running yet.
    }
  }

  async function unpair(deviceId: string) {
    await invoke("mobile_unpair_device", { deviceId });
    await refreshDevices();
  }

  if (!config) return null;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>Source Mobile {phoneConnected ? "· phone connected" : ""}</CardTitle>
          <CardDescription>
            Pair your iPhone to send audio and access your agent conversations over your local network.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="flex items-center justify-between gap-6">
            <div className="space-y-1">
              <Label htmlFor="mobile-enabled" className="text-sm font-medium">Enable mobile server</Label>
              <p className="text-sm text-muted-foreground">
                Let paired devices find and connect to this Mac{port ? ` on port ${port}` : ""}.
              </p>
            </div>
            <Switch
              id="mobile-enabled"
              checked={config.mobile_enabled}
              onCheckedChange={(checked) => controller.updateConfig({ mobile_enabled: checked })}
            />
          </div>

          <div className="space-y-3">
            <Label className="text-sm font-medium">Pair a phone</Label>
            <p className="text-sm text-muted-foreground">
              Open Source Mobile on your iPhone, tap Connect, and point it at this code.
            </p>
            {qr ? (
              <div className="space-y-2">
                <div
                  className="w-fit rounded-xl border bg-white p-3"
                  aria-label="Pairing QR code"
                  dangerouslySetInnerHTML={{ __html: qr }}
                />
                <p className="text-xs text-muted-foreground">
                  Expires in 5 minutes. Anyone who scans it can pair, so don't screenshot or share it.
                </p>
                <Button
                  type="button"
                  onClick={() => {
                    setQr(null);
                    void invoke("mobile_clear_pairing_qr").catch(() => {});
                  }}
                  variant="outline" size="sm"
                >
                  Hide code
                </Button>
              </div>
            ) : (
              <Button
                type="button"
                onClick={showQr}
                size="sm"
              >
                Show pairing code
              </Button>
            )}
            {justPaired ? (
              <p className="text-sm text-emerald-600 dark:text-emerald-400">{justPaired} paired.</p>
            ) : null}
            {qrError ? <p className="text-sm text-destructive">{qrError}</p> : null}
          </div>

          {requests.length > 0 ? (
            <div className="space-y-2">
              <Label className="text-sm font-medium">Waiting for approval</Label>
              <p className="text-sm text-muted-foreground">
                This phone couldn't use the camera, so check the code matches instead.
              </p>
              <div className="space-y-2">
                {requests.map((request) => (
                  <div
                    key={request.requestId}
                    className="space-y-3 rounded-md border border-blue-500/40 bg-blue-500/5 px-3 py-3"
                  >
                    <div className="text-sm">
                      <span className="font-medium">{request.deviceName}</span> wants to connect.
                    </div>
                    <div className="text-sm text-muted-foreground">
                      Only allow this if your iPhone is showing the same code:
                    </div>
                    <div className="font-mono text-3xl tracking-[0.3em]">{request.sas}</div>
                    <div className="flex items-center gap-2">
                      <Button
                        type="button"
                        onClick={() => resolveRequest(request.requestId, true)}
                        size="sm"
                      >
                        Allow
                      </Button>
                      <Button
                        type="button"
                        onClick={() => resolveRequest(request.requestId, false)}
                        variant="outline" size="sm"
                      >
                        Deny
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ) : null}

          <div className="space-y-2">
            <Label className="text-sm font-medium">Paired devices</Label>
            {devices.length === 0 ? (
              <p className="text-sm text-muted-foreground">No phones paired yet.</p>
            ) : (
              <div className="space-y-2">
                {devices.map((device) => (
                  <div key={device.deviceId} className="flex items-center justify-between rounded-md border px-3 py-2">
                    <div>
                      <div className="text-sm font-medium">{device.deviceName || device.deviceId}</div>
                      <div className="text-xs text-muted-foreground">
                        Last seen {new Date(device.lastSeenAt).toLocaleString()}
                      </div>
                    </div>
                    <Button
                      type="button"
                      onClick={() => unpair(device.deviceId)}
                      variant="outline" size="sm"
                    >
                      Unpair
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="space-y-2">
            <Label htmlFor="mobile-retention" className="text-sm font-medium">Clip retention (days, 0 = forever)</Label>
            <Input
              id="mobile-retention"
              type="number"
              min={0}
              max={3650}
              value={config.mobile_clip_retention_days}
              onChange={(event) =>
                controller.updateConfig({ mobile_clip_retention_days: Number(event.target.value) })
              }
              className="w-24"
            />
          </div>
          <Disclosure title="Agent permissions" description="Control whether your phone can send prompts and use Bypass permissions.">
            <div className="space-y-6">
          <div className="flex items-center justify-between gap-6">
            <div className="space-y-1">
              <Label htmlFor="mobile-agent-prompts" className="text-sm font-medium">
                Let the phone send prompts to coding agents
              </Label>
              <p className="max-w-[60ch] text-sm text-muted-foreground">
                Your paired phone can already see your agent sessions. Turn this on to also let it
                send prompts into them, which can change files on this Mac. Takes effect right away.
              </p>
            </div>
            <Switch
              id="mobile-agent-prompts"
              checked={config.mobile_agent_prompts_enabled ?? false}
              onCheckedChange={(checked) => void setPhonePrompts(checked)}
            />
          </div>

          <div className="flex items-center justify-between gap-6">
            <div className="space-y-1">
              <Label htmlFor="agent-prompts-keep-bypass" className="text-sm font-medium">
                Keep Bypass permissions for prompts from the phone
              </Label>
              <p className="max-w-[60ch] text-sm text-muted-foreground">
                The phone can't show Claude's permission requests, so by default anything that needs
                your approval is refused for prompts sent from it. Turn this on to let a conversation set to
                Bypass permissions on this Mac keep that mode: Claude then runs commands and changes
                files without asking, for anyone using your paired phone. Also applies to prompts
                sent from SOURCE's Agents tab. Other conversations are unaffected.
              </p>
            </div>
            <Switch
              id="agent-prompts-keep-bypass"
              checked={config.agent_prompts_keep_bypass ?? false}
              onCheckedChange={(checked) => void setKeepBypass(checked)}
            />
          </div>

            </div>
          </Disclosure>
          <MobileConnectionDetails fingerprint={fingerprint} />
        </CardContent>
      </Card>
    </div>
  );
}
