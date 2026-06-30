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
        <CardTitle>PII Detection</CardTitle>
        <CardDescription>
          Detect-only is the default. Review surfaces exist so you can inspect what SOURCE identified before any future automation is added.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-6">
        <div className="flex items-center justify-between">
          <div className="space-y-1">
            <Label className="text-base">Enable PII detection</Label>
            <p className="text-sm text-muted-foreground">
              Keep personal-data detection active in review flows.
            </p>
          </div>
          <Switch
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
            <Label className="text-base">Detect only</Label>
            <p className="text-sm text-muted-foreground">
              Do not auto-redact or auto-drop content yet.
            </p>
          </div>
          <Switch
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
          <Label className="text-base">Entity categories</Label>
          <div className="grid gap-3 md:grid-cols-2">
            {PII_CATEGORIES.map((category) => (
              <div
                key={category.value}
                className="flex items-center justify-between rounded-lg border border-border/70 px-3 py-3"
              >
                <span className="text-sm">{category.label}</span>
                <Switch
                  checked={config.pii_settings.enabled_categories.includes(category.value)}
                  onCheckedChange={(enabled) => controller.updatePiiCategory(category.value, enabled)}
                />
              </div>
            ))}
          </div>
        </div>

        <div className="space-y-2">
          <Label className="text-base">Review confidence threshold</Label>
          <Input
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
