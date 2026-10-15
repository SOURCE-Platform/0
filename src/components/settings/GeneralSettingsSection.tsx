import { Moon, Sun, Monitor } from "lucide-react";
import { Button } from "@/components/ui/button";
import { SettingsPanel, SettingRow } from "@/components/ui/page";
import { Switch } from "@/components/ui/switch";
import { SettingsController } from "@/components/settings/useSettingsController";

export function GeneralSettingsSection({ controller }: { controller: SettingsController }) {
  const { config } = controller;
  if (!config) return null;
  const themes = [
    { value: "light", label: "Light", Icon: Sun },
    { value: "dark", label: "Dark", Icon: Moon },
    { value: "system", label: "System", Icon: Monitor },
  ] as const;

  return (
    <div className="space-y-4">
      <SettingsPanel title="Appearance">
        <div className="setting-row flex-wrap">
          <span className="text-sm font-medium">Theme</span>
          <div className="flex gap-1 rounded-lg border p-1" role="group" aria-label="Theme">
            {themes.map(({ value, label, Icon }) => (
              <Button key={value} size="sm" variant={controller.theme === value ? "secondary" : "ghost"}
                aria-pressed={controller.theme === value} onClick={() => controller.setTheme(value)}>
                <Icon className="h-4 w-4" />{label}
              </Button>
            ))}
          </div>
        </div>
        <SettingRow title="Show descriptions" htmlFor="show-descriptions" description="Keep helpful explanations visible throughout the app.">
          <Switch id="show-descriptions" checked={controller.showDescriptions} onCheckedChange={controller.setShowDescriptions} />
        </SettingRow>
      </SettingsPanel>
      <SettingsPanel title="Startup">
        <SettingRow title="Launch at login" htmlFor="auto-start" description="Open SOURCE automatically when you sign in to this Mac.">
          <Switch id="auto-start" checked={config.auto_start} onCheckedChange={(auto_start) => controller.updateConfig({ auto_start })} />
        </SettingRow>
      </SettingsPanel>
      <div className="flex flex-wrap items-center justify-between gap-4 px-1 pt-3">
        <p className="text-xs text-muted-foreground">Reset returns all settings to their defaults.</p>
        <Button variant="outline" size="sm" onClick={controller.resetToDefaults} disabled={controller.saving}>Reset to defaults</Button>
      </div>
    </div>
  );
}
