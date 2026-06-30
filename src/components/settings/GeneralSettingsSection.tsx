import { Moon, Sun, Monitor } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { SettingsController } from "@/components/settings/useSettingsController";

export function GeneralSettingsSection({ controller }: { controller: SettingsController }) {
  const { config } = controller;
  if (!config) return null;

  const themeOptions = [
    { value: "light", label: "Light", icon: <Sun className="h-4 w-4" /> },
    { value: "dark", label: "Dark", icon: <Moon className="h-4 w-4" /> },
    { value: "system", label: "System", icon: <Monitor className="h-4 w-4" /> },
  ] as const;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>Data Mode</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-center gap-4">
            <span
              className={`text-sm font-medium transition-colors ${
                !config.mock_data_mode ? "text-foreground" : "text-muted-foreground"
              }`}
            >
              Real
            </span>
            <Switch
              id="mock-mode"
              checked={config.mock_data_mode}
              onCheckedChange={controller.handleMockDataModeChange}
              disabled={controller.saving}
              className="data-[state=checked]:bg-blue-500 data-[state=unchecked]:bg-blue-500 dark:data-[state=unchecked]:bg-blue-500"
            />
            <span
              className={`text-sm font-medium transition-colors ${
                config.mock_data_mode ? "text-foreground" : "text-muted-foreground"
              }`}
            >
              Mock
            </span>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Application Behavior</CardTitle>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="flex items-center justify-between">
            <div className="space-y-1">
              <Label htmlFor="auto-start" className="text-base">
                Launch on system startup
              </Label>
              <p className="text-sm text-muted-foreground">
                Open SOURCE automatically when the computer boots.
              </p>
            </div>
            <Switch
              id="auto-start"
              checked={config.auto_start}
              onCheckedChange={(checked) => controller.updateConfig({ auto_start: checked })}
            />
          </div>

          <div className="space-y-3">
            <Label className="text-base">Theme</Label>
            <div className="flex w-fit overflow-hidden rounded-lg border">
              {themeOptions.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => controller.setTheme(option.value)}
                  className={`flex items-center gap-2 px-4 py-2 text-sm ${
                    controller.theme === option.value
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:bg-muted"
                  }`}
                >
                  {option.icon}
                  {option.label}
                </button>
              ))}
            </div>
          </div>

          <div className="flex items-center justify-between">
            <div className="space-y-1">
              <Label htmlFor="show-descriptions" className="text-base">
                Show descriptions
              </Label>
              <p className="text-sm text-muted-foreground">
                Keep explanatory text visible throughout the app.
              </p>
            </div>
            <Switch
              id="show-descriptions"
              checked={controller.showDescriptions}
              onCheckedChange={controller.setShowDescriptions}
            />
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
