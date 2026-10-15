import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { PII_CATEGORIES } from "@/components/settings/constants";
import { SettingsController } from "@/components/settings/useSettingsController";

export function PrivacySettingsSection({ controller }: { controller: SettingsController }) {
  const { config } = controller;
  if (!config) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Personal data detection</CardTitle>
        <CardDescription>
          Identify personal information in captured text and choose which categories to flag.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        <div className="flex items-center justify-between">
          <div className="space-y-1">
            <Label className="text-sm font-medium">Enable personal data detection</Label>
            <p className="text-sm text-muted-foreground">
              Keep personal-data detection active in review flows.
            </p>
          </div>
          <Switch
            aria-label="Enable personal data detection"
            checked={config.pii_settings.enabled}
            onCheckedChange={(enabled) =>
              controller.updateConfig({
                pii_settings: {
                  ...config.pii_settings,
                  enabled,
                },
              })
            }
          />
        </div>

        <div className="flex items-center justify-between">
          <div className="space-y-1">
            <Label className="text-sm font-medium">Detect only</Label>
            <p className="text-sm text-muted-foreground">
              Flag personal information without changing or removing the captured text.
            </p>
          </div>
          <Switch
            aria-label="Detect only"
            checked={config.pii_settings.detect_only}
            onCheckedChange={(detect_only) =>
              controller.updateConfig({
                pii_settings: {
                  ...config.pii_settings,
                  detect_only,
                },
              })
            }
          />
        </div>

        <div className="space-y-3">
          <Label className="text-sm font-medium">Entity categories</Label>
          <div className="grid gap-3 md:grid-cols-2">
            {PII_CATEGORIES.map((category) => (
              <div
                key={category.value}
                className="flex items-center justify-between rounded-lg border border-border/70 px-3 py-3"
              >
                <span className="text-sm">{category.label}</span>
                <Switch
                  aria-label={category.label}
                  checked={config.pii_settings.enabled_categories.includes(category.value)}
                  onCheckedChange={(enabled) => controller.updatePiiCategory(category.value, enabled)}
                />
              </div>
            ))}
          </div>
        </div>

        <div className="space-y-2">
          <Label className="text-sm font-medium">Review confidence threshold</Label>
          <Input
            aria-label="Review confidence threshold"
            className="max-w-32"
            type="number"
            min="0"
            max="1"
            step="0.05"
            value={config.pii_settings.review_confidence_threshold}
            onChange={(event) =>
              controller.updateConfig({
                pii_settings: {
                  ...config.pii_settings,
                  review_confidence_threshold: Number.parseFloat(event.target.value) || 0,
                },
              })
            }
          />
        </div>
      </CardContent>
    </Card>
  );
}
