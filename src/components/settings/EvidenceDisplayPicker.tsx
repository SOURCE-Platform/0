import { Monitor } from "lucide-react";
import { SettingsController } from "@/components/settings/useSettingsController";

/**
 * One connected screen: shown as a fact, nothing to choose. Several: a
 * row of radio-style buttons (a laptop plus one monitor is the usual
 * case). No dropdown either way.
 */
export function EvidenceDisplayPicker({ controller }: { controller: SettingsController }) {
  const { displays, selectedDisplay } = controller;

  if (displays.length === 0) {
    return <p className="text-sm text-muted-foreground">No display detected.</p>;
  }

  if (displays.length === 1) {
    const display = displays[0];
    return (
      <div className="flex items-center gap-2 text-sm text-foreground">
        <Monitor className="h-4 w-4 text-muted-foreground" />
        <span>
          {display.name}{" "}
          <span className="text-muted-foreground">
            ({display.width}×{display.height})
          </span>
        </span>
      </div>
    );
  }

  return (
    <div role="radiogroup" aria-label="Evidence display" className="flex flex-wrap gap-2">
      {displays.map((display) => {
        const selected = String(display.id) === selectedDisplay;
        return (
          <button
            key={display.id}
            type="button"
            role="radio"
            aria-checked={selected}
            onClick={() => controller.selectDisplay(String(display.id))}
            className={`flex items-center gap-2 rounded-lg border px-4 py-2 text-sm transition-colors ${
              selected
                ? "border-white/30 bg-white/8 text-foreground"
                : "border-border text-muted-foreground hover:bg-muted/30 hover:text-foreground"
            }`}
          >
            <Monitor className="h-4 w-4" />
            {display.name}
            <span className="text-muted-foreground">
              ({display.width}×{display.height})
            </span>
          </button>
        );
      })}
    </div>
  );
}
