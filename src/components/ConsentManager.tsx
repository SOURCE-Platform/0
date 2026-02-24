import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import { Plus, AppWindow } from "lucide-react";
import { cn } from "@/lib/utils";

const PII_FILTERS: { key: string; label: string }[] = [
  { key: "names",        label: "Names" },
  { key: "emails",       label: "Email addresses" },
  { key: "phone",        label: "Phone numbers" },
  { key: "passwords",    label: "Passwords" },
  { key: "credit_cards", label: "Credit card numbers" },
  { key: "ssn",          label: "SSN / Gov IDs" },
  { key: "addresses",    label: "Addresses" },
  { key: "ip_addresses", label: "IP addresses" },
];

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

  const [piiFilters, setPiiFilters] = useState<Record<string, boolean>>(
    () => Object.fromEntries(PII_FILTERS.map(p => [p.key, false]))
  );

  const [websiteBlacklist, setWebsiteBlacklist] = useState<string[]>([]);
  const [newWebsite, setNewWebsite] = useState("");
  const [websiteDupe, setWebsiteDupe] = useState(false);
  const [appBlacklist, setAppBlacklist] = useState<string[]>([]);
  const [newApp, setNewApp] = useState("");
  const [appDupe, setAppDupe] = useState(false);

  useEffect(() => { loadConsents(); loadBlacklists(); }, []);

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

  async function loadBlacklists() {
    try {
      const config = await invoke<{ website_blacklist: string[]; app_blacklist: string[] }>("get_config");
      setWebsiteBlacklist(config.website_blacklist ?? []);
      setAppBlacklist(config.app_blacklist ?? []);
    } catch (e) {
      console.error("Failed to load blacklists:", e);
    }
  }

  async function saveBlacklists(websites: string[], apps: string[]) {
    try {
      const config = await invoke<Record<string, unknown>>("get_config");
      await invoke("update_config", { config: { ...config, website_blacklist: websites, app_blacklist: apps } });
    } catch (e) {
      console.error("Failed to save blacklists:", e);
    }
  }

  function addWebsite() {
    const url = newWebsite.trim();
    if (!url) return;
    if (websiteBlacklist.some(u => u.toLowerCase() === url.toLowerCase())) {
      setWebsiteDupe(true);
      return;
    }
    const next = [...websiteBlacklist, url];
    setWebsiteBlacklist(next);
    setNewWebsite("");
    setWebsiteDupe(false);
    saveBlacklists(next, appBlacklist);
  }

  function removeWebsite(i: number) {
    const next = websiteBlacklist.filter((_, j) => j !== i);
    setWebsiteBlacklist(next);
    saveBlacklists(next, appBlacklist);
  }

  function updateWebsite(i: number, value: string) {
    const next = [...websiteBlacklist];
    next[i] = value;
    setWebsiteBlacklist(next);
    saveBlacklists(next, appBlacklist);
  }

  function addApp() {
    const name = newApp.trim();
    if (!name) return;
    if (appBlacklist.some(a => a.toLowerCase() === name.toLowerCase())) {
      setAppDupe(true);
      return;
    }
    const next = [...appBlacklist, name];
    setAppBlacklist(next);
    setNewApp("");
    setAppDupe(false);
    saveBlacklists(websiteBlacklist, next);
  }

  function removeApp(i: number) {
    const next = appBlacklist.filter((_, j) => j !== i);
    setAppBlacklist(next);
    saveBlacklists(websiteBlacklist, next);
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-[200px]">
        <p className="text-muted-foreground text-sm">Loading...</p>
      </div>
    );
  }

  return (
    <div className="flex gap-16 pt-2">

      {/* ── Col 1: Record these ── */}
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

      {/* ── Col 2: Filter PII ── */}
      <div className="shrink-0">
        <h2 className="text-base text-foreground mb-6">Filter PII</h2>
        <div className="space-y-5">
          {PII_FILTERS.map(p => {
            const isOn = piiFilters[p.key];
            return (
              <div key={p.key} className="flex items-center gap-3">
                <Switch
                  checked={isOn}
                  onCheckedChange={() => setPiiFilters(prev => ({ ...prev, [p.key]: !prev[p.key] }))}
                />
                <span className={cn(
                  "text-sm",
                  isOn ? "text-foreground" : "text-muted-foreground"
                )}>
                  {p.label}
                </span>
              </div>
            );
          })}
        </div>
      </div>

      {/* ── Col 3: Don't record these ── */}
      <div className="flex-1 min-w-0">
        <h2 className="text-base text-foreground mb-6">
          <span className="underline decoration-dashed decoration-2 underline-offset-[7px] decoration-[#FF0033] dark:decoration-[#FF879F]">Don't</span> record these
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
                    onChange={e => updateWebsite(i, e.target.value)}
                    onBlur={() => saveBlacklists(websiteBlacklist, appBlacklist)}
                    className="h-8 text-sm"
                  />
                  <button
                    onClick={() => removeWebsite(i)}
                    className="shrink-0 cursor-pointer h-8 w-8 flex items-center justify-center rounded-md text-muted-foreground hover:text-[#FF0033] dark:hover:text-[#FF879F] hover:bg-black/[8%] dark:hover:bg-white/[8%] transition-colors"
                  >
                    <Plus className="size-4 rotate-45" />
                  </button>
                </div>
              ))}
              <div className="space-y-1">
                <div className="flex items-center gap-2">
                  <Input
                    placeholder="Paste URL"
                    value={newWebsite}
                    onChange={e => { setNewWebsite(e.target.value); setWebsiteDupe(false); }}
                    onKeyDown={e => e.key === "Enter" && addWebsite()}
                    className={cn("h-8 text-sm", websiteDupe && "border-[#FF0033] dark:border-[#FF879F] focus-visible:ring-[#FF0033]/30")}
                  />
                  <button
                    onClick={addWebsite}
                    className="shrink-0 cursor-pointer h-8 w-8 flex items-center justify-center rounded-md text-muted-foreground hover:text-[#0077FF] dark:hover:text-[#67C0FF] hover:bg-black/[8%] dark:hover:bg-white/[8%] transition-colors"
                  >
                    <Plus className="size-4" />
                  </button>
                </div>
                {websiteDupe && (
                  <p className="text-xs text-[#FF0033] dark:text-[#FF879F] pl-1">Already in the list</p>
                )}
              </div>
            </div>
          </div>

          {/* Apps */}
          <div className="w-52 shrink-0">
            <p className="text-sm text-muted-foreground mb-3">Apps</p>
            <div className="space-y-2">
              {appBlacklist.map((app, i) => (
                <div key={i} className="flex items-center gap-2">
                  <AppWindow className="size-4 text-muted-foreground shrink-0" />
                  <span className="text-sm flex-1 truncate">{app}</span>
                  <button
                    onClick={() => removeApp(i)}
                    className="shrink-0 cursor-pointer h-8 w-8 flex items-center justify-center rounded-md text-muted-foreground hover:text-[#FF0033] dark:hover:text-[#FF879F] hover:bg-black/[8%] dark:hover:bg-white/[8%] transition-colors"
                  >
                    <Plus className="size-4 rotate-45" />
                  </button>
                </div>
              ))}
              <div className="space-y-1">
                <div className="flex items-center gap-2">
                  <Input
                    placeholder="App name"
                    value={newApp}
                    onChange={e => { setNewApp(e.target.value); setAppDupe(false); }}
                    onKeyDown={e => e.key === "Enter" && addApp()}
                    className={cn("h-8 text-sm", appDupe && "border-[#FF0033] dark:border-[#FF879F] focus-visible:ring-[#FF0033]/30")}
                  />
                  <button
                    onClick={addApp}
                    className="shrink-0 cursor-pointer h-8 w-8 flex items-center justify-center rounded-md text-muted-foreground hover:text-[#0077FF] dark:hover:text-[#67C0FF] hover:bg-black/[8%] dark:hover:bg-white/[8%] transition-colors"
                  >
                    <Plus className="size-4" />
                  </button>
                </div>
                {appDupe && (
                  <p className="text-xs text-[#FF0033] dark:text-[#FF879F] pl-1">Already in the list</p>
                )}
              </div>
            </div>
          </div>

        </div>
      </div>

    </div>
  );
}
