import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Plus, AppWindow } from "lucide-react";
import { cn } from "@/lib/utils";

interface ConsentState {
  screen_recording: boolean;
  os_activity: boolean;
  keyboard_recording: boolean;
  mouse_recording: boolean;
  camera_recording: boolean;
  microphone_recording: boolean;
}

type ConsentKey = keyof ConsentState;

const FEATURES: { key: ConsentKey | null; label: string; disabled?: boolean }[] = [
  { key: "screen_recording",    label: "Screen" },
  { key: "os_activity",         label: "Session info" },
  { key: null,                  label: "Background processes", disabled: true },
  { key: "keyboard_recording",  label: "Keyboard" },
  { key: "mouse_recording",     label: "Mouse" },
  { key: "camera_recording",    label: "Camera" },
  { key: "microphone_recording",label: "Mic" },
];

export default function ConsentManager() {
  const [consents, setConsents] = useState<ConsentState>({
    screen_recording: false,
    os_activity: false,
    keyboard_recording: false,
    mouse_recording: false,
    camera_recording: false,
    microphone_recording: false,
  });
  const [loading, setLoading] = useState(true);
  const [updating, setUpdating] = useState<string | null>(null);

  const [websiteBlacklist, setWebsiteBlacklist] = useState<string[]>([]);
  const [newWebsite, setNewWebsite] = useState("");
  const [appBlacklist, setAppBlacklist] = useState<string[]>([]);
  const [newApp, setNewApp] = useState("");

  useEffect(() => { loadConsents(); }, []);

  async function loadConsents() {
    try {
      const all = await invoke<Record<string, boolean>>("get_all_consents");
      setConsents({
        screen_recording:     all.screen_recording     ?? false,
        os_activity:          all.os_activity          ?? false,
        keyboard_recording:   all.keyboard_recording   ?? false,
        mouse_recording:      all.mouse_recording      ?? false,
        camera_recording:     all.camera_recording     ?? false,
        microphone_recording: all.microphone_recording ?? false,
      });
    } catch (e) {
      console.error("Failed to load consents:", e);
    } finally {
      setLoading(false);
    }
  }

  async function toggleConsent(key: ConsentKey) {
    setUpdating(key);
    const current = consents[key];
    try {
      await invoke(current ? "revoke_consent" : "request_consent", { feature: key });
      setConsents(prev => ({ ...prev, [key]: !current }));
    } catch (e) {
      console.error(`Failed to toggle ${key}:`, e);
    } finally {
      setUpdating(null);
    }
  }

  function addWebsite() {
    const url = newWebsite.trim();
    if (url) { setWebsiteBlacklist(p => [...p, url]); setNewWebsite(""); }
  }

  function addApp() {
    const name = newApp.trim();
    if (name) { setAppBlacklist(p => [...p, name]); setNewApp(""); }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-[200px]">
        <p className="text-muted-foreground text-sm">Loading...</p>
      </div>
    );
  }

  return (
    <div className="flex gap-20 pt-2">

      {/* ── Left: Record these ── */}
      <div className="shrink-0">
        <h2 className="text-base text-foreground mb-6">Record these</h2>
        <div className="space-y-5">
          {FEATURES.map(f => {
            const isOn = f.key ? consents[f.key] : false;
            return (
              <div key={f.label} className="flex items-center gap-3">
                <Switch
                  checked={isOn}
                  onCheckedChange={() => f.key && toggleConsent(f.key)}
                  disabled={f.disabled || updating === f.key}
                />
                <span className={cn(
                  "text-sm",
                  f.disabled || (!isOn && !f.disabled) ? "text-muted-foreground" : "text-foreground"
                )}>
                  {f.label}
                </span>
              </div>
            );
          })}
        </div>
      </div>

      {/* ── Right: Don't record these ── */}
      <div className="flex-1 min-w-0">
        <h2 className="text-base text-foreground mb-6">
          <span className="underline decoration-1 underline-offset-[7px] decoration-[#FF0033] dark:decoration-[#FF879F]">Don't</span> record these
        </h2>

        <div className="flex gap-12">

          {/* Websites */}
          <div className="flex-1 min-w-0">
            <p className="text-sm text-muted-foreground mb-3">Websites</p>
            <div className="space-y-2">
              {websiteBlacklist.map((url, i) => (
                <div key={i} className="flex items-center gap-2">
                  <Input
                    value={url}
                    onChange={e => {
                      const next = [...websiteBlacklist];
                      next[i] = e.target.value;
                      setWebsiteBlacklist(next);
                    }}
                    className="h-8 text-sm"
                  />
                  <button
                    onClick={() => setWebsiteBlacklist(p => p.filter((_, j) => j !== i))}
                    className="shrink-0 cursor-pointer h-8 w-8 flex items-center justify-center rounded-md text-muted-foreground hover:text-[#FF0033] dark:hover:text-[#FF879F] hover:bg-black/[8%] dark:hover:bg-white/[8%] transition-colors"
                  >
                    <Plus className="size-4 rotate-45" />
                  </button>
                </div>
              ))}
              {/* Add row */}
              <div className="flex items-center gap-2">
                <Input
                  placeholder="Paste URL"
                  value={newWebsite}
                  onChange={e => setNewWebsite(e.target.value)}
                  onKeyDown={e => e.key === "Enter" && addWebsite()}
                  className="h-8 text-sm"
                />
                <button
                  onClick={addWebsite}
                  className="shrink-0 cursor-pointer h-8 w-8 flex items-center justify-center rounded-md text-muted-foreground hover:text-[#0077FF] dark:hover:text-[#67C0FF] hover:bg-black/[8%] dark:hover:bg-white/[8%] transition-colors"
                >
                  <Plus className="size-4" />
                </button>
              </div>
            </div>
          </div>

          {/* Apps */}
          <div className="w-48 shrink-0">
            <p className="text-sm text-muted-foreground mb-3">Apps</p>
            <div className="space-y-2">
              {appBlacklist.map((app, i) => (
                <div key={i} className="flex items-center gap-2">
                  <AppWindow className="size-4 text-muted-foreground shrink-0" />
                  <span className="text-sm flex-1 truncate">{app}</span>
                  <button
                    onClick={() => setAppBlacklist(p => p.filter((_, j) => j !== i))}
                    className="shrink-0 cursor-pointer h-8 w-8 flex items-center justify-center rounded-md text-muted-foreground hover:text-[#FF0033] dark:hover:text-[#FF879F] hover:bg-black/[8%] dark:hover:bg-white/[8%] transition-colors"
                  >
                    <Plus className="size-4 rotate-45" />
                  </button>
                </div>
              ))}
              {/* Add row */}
              <div className="flex items-center gap-2">
                <Input
                  placeholder="App name"
                  value={newApp}
                  onChange={e => setNewApp(e.target.value)}
                  onKeyDown={e => e.key === "Enter" && addApp()}
                  className="h-8 text-sm"
                />
                <button
                  onClick={addApp}
                  className="shrink-0 cursor-pointer h-8 w-8 flex items-center justify-center rounded-md text-muted-foreground hover:text-[#0077FF] dark:hover:text-[#67C0FF] hover:bg-black/[8%] dark:hover:bg-white/[8%] transition-colors"
                >
                  <Plus className="size-4" />
                </button>
              </div>
            </div>
          </div>

        </div>
      </div>

    </div>
  );
}
