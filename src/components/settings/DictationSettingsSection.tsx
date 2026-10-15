import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SettingsPanel, SettingRow } from "@/components/ui/page";
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
      <SettingsPanel title="Dictation overlay" description="The listening pill appears while you dictate with Right Option.">
        <SettingRow title="Show running words" htmlFor="dictation-words" description="Show the live transcript below the waveform. Takes effect on your next dictation.">
          <Switch id="dictation-words" checked={wordsVisible} onCheckedChange={handleToggle} />
        </SettingRow>
        {note ? <p className="mt-2 text-xs text-destructive">{note}</p> : null}
      </SettingsPanel>

      <SettingsPanel title="Speech recognition" description="Parakeet v3 transcribes your speech locally on this Mac.">
        <p className="text-sm leading-6 text-muted-foreground">If your words aren’t pasted into an app, open a dictation block in Timeline and choose Copy text.</p>
      </SettingsPanel>
    </div>
  );
}
