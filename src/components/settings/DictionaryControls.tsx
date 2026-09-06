import { useState } from "react";
import { SettingsController } from "@/components/settings/useSettingsController";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export function DictionaryControls({
  controller,
}: {
  controller: SettingsController;
}) {
  const { config } = controller;
  const [heardAs, setHeardAs] = useState("");
  const [writeAs, setWriteAs] = useState("");

  if (!config) return null;
  const entries = config.custom_dictionary ?? [];

  function handleAdd() {
    controller.addDictionaryEntry(heardAs, writeAs);
    setHeardAs("");
    setWriteAs("");
  }

  return (
    <div className="space-y-3 rounded-lg border p-3">
      <div>
        <Label className="text-sm">My words</Label>
        <p className="max-w-[60ch] text-xs leading-5 text-muted-foreground">
          When transcription mishears a name or term, add it here.
          O swaps what it heard for how you spell it.
        </p>
      </div>

      {entries.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          No custom words yet.
        </p>
      ) : (
        <ul className="space-y-1.5">
          {entries.map((entry, index) => (
            <li
              key={`${entry.replacement}-${index}`}
              className="flex items-center justify-between gap-2 rounded-md bg-muted/50 px-2 py-1.5 text-xs"
            >
              <span className="truncate">
                <span className="text-muted-foreground">
                  {entry.triggers.join(", ")}
                </span>
                <span className="mx-1.5">→</span>
                <span className="font-medium">{entry.replacement}</span>
              </span>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => controller.removeDictionaryEntry(index)}
              >
                Remove
              </Button>
            </li>
          ))}
        </ul>
      )}

      <div className="grid gap-2 sm:grid-cols-2">
        <div className="space-y-1">
          <Label htmlFor="dictation-heard-as" className="text-xs">
            Heard as
          </Label>
          <Input
            id="dictation-heard-as"
            value={heardAs}
            onChange={(event) => setHeardAs(event.target.value)}
            placeholder="cub rick"
          />
        </div>
        <div className="space-y-1">
          <Label htmlFor="dictation-write-as" className="text-xs">
            Write as
          </Label>
          <Input
            id="dictation-write-as"
            value={writeAs}
            onChange={(event) => setWriteAs(event.target.value)}
            placeholder="Kubrick"
          />
        </div>
      </div>
      <Button type="button" size="sm" variant="secondary" onClick={handleAdd}>
        Add word
      </Button>
    </div>
  );
}
