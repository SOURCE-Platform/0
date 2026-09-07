import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";

export function DictationSettingsSection() {
  const [wordsVisible, setWordsVisible] = useState(true);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    invoke<boolean>("get_dictation_words_visible")
      .then(setWordsVisible)
      .catch(() => setWordsVisible(true));
  }, []);

  async function handleToggle(enabled: boolean) {
    setWordsVisible(enabled);
    try {
      await invoke("set_dictation_words_visible", { visible: enabled });
      setNote(null);
    } catch (error) {
      setNote(`Could not save: ${error}`);
    }
  }

  return (
    <div className="space-y-4">
      <section className="rounded-lg border border-border/60 bg-muted/15 p-3">
        <Label className="text-sm">Listening pill</Label>
        <p className="mt-1 max-w-[52ch] text-xs leading-5 text-muted-foreground">
          Controls the overlay that appears while you dictate with
          Right Option. Applies to the next dictation.
        </p>
        <div className="mt-3 flex items-center justify-between gap-4">
          <div>
            <Label className="text-sm">Show running words</Label>
            <p className="text-xs text-muted-foreground">
              Live transcript under the waveform. Hide it when it covers
              what you are reading.
            </p>
          </div>
          <Switch checked={wordsVisible} onCheckedChange={handleToggle} />
        </div>
        {note ? <p className="mt-2 text-xs text-destructive">{note}</p> : null}
      </section>

      <section className="rounded-lg border border-border/60 bg-muted/15 p-3">
        <Label className="text-sm">Voice engine</Label>
        <p className="mt-1 max-w-[52ch] text-xs leading-5 text-muted-foreground">
          Parakeet v3 (multilingual) via the bundled helper, sharing one
          model copy with FluidVoice. Model choice and per-language
          benchmarking arrive in a later pass.
        </p>
      </section>
    </div>
  );
}
