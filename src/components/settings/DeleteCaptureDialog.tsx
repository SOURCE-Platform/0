import { Button } from "@/components/ui/button";
import { SettingsController } from "@/components/settings/useSettingsController";

export function DeleteCaptureDialog({ controller }: { controller: SettingsController }) {
  if (!controller.pendingDelete) return null;

  return (
    <div className="fixed inset-0 z-[220] flex items-center justify-center bg-black/55 px-6 backdrop-blur-sm">
      <div className="w-full max-w-xl rounded-2xl border border-border/70 bg-card p-6 shadow-2xl">
        <h2 className="text-2xl font-semibold text-foreground">Confirm deletion</h2>
        <p className="mt-3 max-w-[60ch] text-sm leading-6 text-muted-foreground">
          {controller.pendingDelete.channel
            ? `Delete all stored data for ${controller.pendingDelete.label}? This cannot be undone.`
            : "Delete all capture data across every channel? This cannot be undone."}
        </p>
        <div className="mt-6 flex flex-wrap justify-end gap-3">
          <Button
            type="button"
            variant="outline"
            onClick={() => controller.setPendingDelete(null)}
            disabled={controller.deletingKey !== null}
          >
            Cancel
          </Button>
          <Button
            type="button"
            variant="destructive"
            onClick={controller.confirmDeleteData}
            disabled={controller.deletingKey !== null}
          >
            Delete
          </Button>
        </div>
      </div>
    </div>
  );
}
