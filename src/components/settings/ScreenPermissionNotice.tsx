import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { showSettingsToast } from "@/components/settings/utils";

// Without the macOS grant, screenshots still arrive but hold only SOURCE's
// own window and the menu bar, so OCR silently reads SOURCE instead of the
// app in front. Say so plainly and link straight to the setting.
export function ScreenPermissionNotice() {
  const openSettings = async () => {
    try {
      await invoke("open_screen_capture_settings");
    } catch (error) {
      showSettingsToast({
        type: "error",
        text: `Could not open System Settings: ${String(error)}`,
      });
    }
  };

  return (
    <div className="max-w-xl space-y-2 rounded-lg border border-amber-400/30 bg-amber-400/[0.07] p-3 text-xs leading-5 text-muted-foreground">
      <p>
        macOS Screen Recording permission is off for SOURCE. Screenshots only
        show SOURCE's own window, so other apps' text can't be read. Turn
        SOURCE on under Screen &amp; System Audio Recording, then quit and
        reopen SOURCE.
      </p>
      <Button type="button" size="sm" variant="outline" onClick={openSettings}>
        Open Screen Recording settings
      </Button>
    </div>
  );
}
