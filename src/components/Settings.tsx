import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import ConsentManager from "@/components/ConsentManager";
import { CaptureSettingsSection } from "@/components/settings/CaptureSettingsSection";
import { DeleteCaptureDialog } from "@/components/settings/DeleteCaptureDialog";
import { GeneralSettingsSection } from "@/components/settings/GeneralSettingsSection";
import { HardwareSettingsSection } from "@/components/settings/HardwareSettingsSection";
import { DictationSettingsSection } from "@/components/settings/DictationSettingsSection";
import { PrivacySettingsSection } from "@/components/settings/PrivacySettingsSection";
import { StorageSettingsSection } from "@/components/settings/StorageSettingsSection";
import { SettingsTab } from "@/components/settings/types";
import { useSettingsController } from "@/components/settings/useSettingsController";

export default function Settings() {
  const controller = useSettingsController();

  // Pill gear button jumps straight here.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen("dictation-open-settings", () => {
      controller.setTab("dictation");
    }).then((stop) => {
      unlisten = stop;
    });
    // Consume a pending jump requested while this view was unmounted.
    try {
      if (localStorage.getItem("open-settings-tab") === "dictation") {
        localStorage.removeItem("open-settings-tab");
        controller.setTab("dictation");
      }
    } catch {
      // Storage unavailable: stay on the default tab.
    }
    return () => unlisten?.();
  }, [controller.setTab]);

  if (controller.loading || !controller.config) {
    return (
      <div className="flex min-h-[400px] items-center justify-center">
        <p className="text-muted-foreground">Loading settings...</p>
      </div>
    );
  }

  return (
    <div className="mx-auto w-full max-w-6xl space-y-6">
      <div className="space-y-1">
        <h1 className="text-3xl font-semibold tracking-tight text-foreground">Settings</h1>
        {controller.showDescriptions ? (
          <p className="text-muted-foreground">
            Configure real capture channels, demo mode, PII review behavior, and validation controls.
          </p>
        ) : null}
      </div>

      <AnimatedTabNav
        tabs={[
          { value: "general", label: "General" },
          { value: "capture", label: "Capture" },
          { value: "privacy", label: "Privacy" },
          { value: "storage", label: "Storage" },
          { value: "hardware", label: "Hardware" },
          { value: "dictation", label: "Dictation" },
        ]}
        value={controller.tab}
        onValueChange={(value) => controller.setTab(value as SettingsTab)}
      />

      {controller.tab === "general" ? <GeneralSettingsSection controller={controller} /> : null}
      {controller.tab === "capture" ? <CaptureSettingsSection controller={controller} /> : null}
      {controller.tab === "privacy" ? (
        <div className="space-y-4">
          <PrivacySettingsSection controller={controller} />
          <ConsentManager />
        </div>
      ) : null}
      {controller.tab === "storage" ? <StorageSettingsSection controller={controller} /> : null}
      {controller.tab === "hardware" ? <HardwareSettingsSection /> : null}
      {controller.tab === "dictation" ? <DictationSettingsSection /> : null}

      <Card>
        <CardHeader>
          <CardTitle>Save Changes</CardTitle>
          <CardDescription>
            Channel controls, PII settings, and capture presets are saved to the local SOURCE configuration.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap items-center justify-between gap-3">
          <p className="text-sm text-muted-foreground">
            Current evidence display: {controller.selectedDisplayName}. Save after changing capture profile or channel mix.
          </p>
          <div className="flex gap-3">
            <button
              type="button"
              onClick={controller.resetToDefaults}
              disabled={controller.saving}
              className="inline-flex items-center justify-center rounded-md border border-input bg-background px-4 py-2 text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground disabled:pointer-events-none disabled:opacity-50"
            >
              Reset to defaults
            </button>
            <button
              type="button"
              onClick={() => controller.saveConfig()}
              disabled={controller.saving}
              className="inline-flex items-center justify-center rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-50"
            >
              {controller.saving ? "Saving..." : "Save settings"}
            </button>
          </div>
        </CardContent>
      </Card>

      <DeleteCaptureDialog controller={controller} />
    </div>
  );
}
