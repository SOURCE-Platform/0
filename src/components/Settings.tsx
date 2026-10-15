import { AnimatedTabNav } from "@/components/ui/animated-tab-nav";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { PageHeader } from "@/components/ui/page";
import ConsentManager from "@/components/ConsentManager";
import { CaptureSettingsSection } from "@/components/settings/CaptureSettingsSection";
import { DeleteCaptureDialog } from "@/components/settings/DeleteCaptureDialog";
import { GeneralSettingsSection } from "@/components/settings/GeneralSettingsSection";
import { HardwareSettingsSection } from "@/components/settings/HardwareSettingsSection";
import { DictationSettingsSection } from "@/components/settings/DictationSettingsSection";
import { MobileSettingsSection } from "@/components/settings/MobileSettingsSection";
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
    <div className="settings-page space-y-6">
      <PageHeader title="Settings" description={controller.showDescriptions ? "Choose what SOURCE captures and how it runs on your devices." : undefined}
        action={<span role="status" className="text-xs text-muted-foreground">{controller.saving ? "Saving…" : "Changes save automatically"}</span>} />

      <div className="settings-nav">
      <AnimatedTabNav
        tabs={[
          { value: "general", label: "General" },
          { value: "capture", label: "Capture" },
          { value: "privacy", label: "Privacy" },
          { value: "storage", label: "Storage" },
          { value: "hardware", label: "Hardware" },
          { value: "dictation", label: "Dictation" },
          { value: "mobile", label: "Mobile" },
        ]}
        value={controller.tab}
        onValueChange={(value) => controller.setTab(value as SettingsTab)}
      />

      </div>
      <div className="settings-body" data-section={controller.tab}>
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
      {controller.tab === "mobile" ? <MobileSettingsSection controller={controller} /> : null}

      </div>

      <DeleteCaptureDialog controller={controller} />
    </div>
  );
}
