import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { SettingsLayout } from "@/components/settings/SettingsLayout";
import ConsentManager from "@/components/ConsentManager";
import { CaptureSettingsSection } from "@/components/settings/CaptureSettingsSection";
import { DeleteCaptureDialog } from "@/components/settings/DeleteCaptureDialog";
import { GeneralSettingsSection } from "@/components/settings/GeneralSettingsSection";
import { HardwareSettingsSection } from "@/components/settings/HardwareSettingsSection";
import { DictationSettingsSection } from "@/components/settings/DictationSettingsSection";
import { MobileSettingsSection } from "@/components/settings/MobileSettingsSection";
import { PrivacySettingsSection } from "@/components/settings/PrivacySettingsSection";
import { StorageSettingsSection } from "@/components/settings/StorageSettingsSection";
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

  return (
    <SettingsLayout loading={controller.loading || !controller.config} saving={controller.saving}
      showDescriptions={controller.showDescriptions} tab={controller.tab} onTabChange={controller.setTab}>
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

      <DeleteCaptureDialog controller={controller} />
    </SettingsLayout>
  );
}
