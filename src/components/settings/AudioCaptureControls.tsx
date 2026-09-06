import { AudioSourceMeter } from "@/components/settings/AudioSourceMeter";
import { DictionaryControls } from "@/components/settings/DictionaryControls";
import { SettingsController } from "@/components/settings/useSettingsController";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";

export function AudioCaptureControls({
  controller,
}: {
  controller: SettingsController;
}) {
  const { config } = controller;
  const defaultSource = controller.audioInputSources.find(
    (source) => source.isSystemDefault,
  );

  if (!config) return null;

  return (
    <div className="max-w-xl space-y-4 pt-2">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-blue-400/25 bg-blue-400/[0.06] p-3">
        <div>
          <Label className="text-sm">Capture all audio context</Label>
          <p className="max-w-[50ch] text-xs leading-5 text-muted-foreground">
            Enables microphone, desktop audio, transcription,
            speech emotion, and sound events.
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="secondary"
          onClick={controller.enableAllAudio}
        >
          Enable all
        </Button>
      </div>

      <MicrophoneControls
        controller={controller}
        defaultSource={defaultSource}
      />
      <DesktopAudioControls controller={controller} />
      <AudioAnalysisControls controller={controller} />
      <DictionaryControls controller={controller} />
    </div>
  );
}

function MicrophoneControls({
  controller,
  defaultSource,
}: {
  controller: SettingsController;
  defaultSource: SettingsController["audioInputSources"][number] | undefined;
}) {
  const { config } = controller;
  if (!config) return null;

  return (
    <section className="rounded-lg border border-border/60 bg-muted/15 p-3">
      <div className="flex items-center justify-between gap-4">
        <div>
          <Label className="text-sm">Record microphone</Label>
          <p className="max-w-[52ch] text-xs leading-5 text-muted-foreground">
            Captures your selected live input for speech, emotion,
            and nearby sound events.
          </p>
        </div>
        <Switch
          checked={config.audio_microphone_enabled}
          onCheckedChange={controller.updateAudioMicrophoneEnabled}
        />
      </div>
      <div className="mt-3 grid gap-3 sm:grid-cols-[minmax(0,1fr)_11rem] sm:items-end">
        <div className="space-y-2">
          <Label className="text-xs uppercase tracking-wide text-muted-foreground">
            Microphone input
          </Label>
          <Select
            value={config.selected_audio_input_id ?? "__auto__"}
            onValueChange={controller.selectAudioInput}
            disabled={!config.audio_microphone_enabled}
          >
            <SelectTrigger className="h-auto min-h-10">
              <SelectValue placeholder="Choose a microphone" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__auto__" className="h-auto py-2">
                <div className="flex flex-col items-start text-left">
                  <span>
                    {defaultSource?.name ?? "No macOS default microphone detected"}
                  </span>
                  {defaultSource ? (
                    <span className="text-xs text-muted-foreground">
                      macOS default
                    </span>
                  ) : null}
                </div>
              </SelectItem>
              {controller.audioInputSources
                .filter((source) => !source.isSystemDefault)
                .map((source) => (
                  <SelectItem key={source.sourceId} value={source.sourceId}>
                    {source.name}
                  </SelectItem>
                ))}
            </SelectContent>
          </Select>
        </div>
        <AudioSourceMeter
          enabled={config.audio_microphone_enabled}
          selectedAudioInputId={config.selected_audio_input_id}
          desktopAudioEnabled={config.audio_desktop_enabled}
          desktopGainDb={config.desktop_audio_gain_db}
          ownsStream={
            config.audio_microphone_enabled || config.audio_desktop_enabled
          }
          source="microphone"
        />
      </div>
    </section>
  );
}

function DesktopAudioControls({ controller }: { controller: SettingsController }) {
  const { config } = controller;
  if (!config) return null;

  return (
    <section className="rounded-lg border border-border/60 bg-muted/15 p-3">
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_11rem_auto] sm:items-center">
        <div>
          <Label className="text-sm">Record desktop audio</Label>
          <p className="max-w-[54ch] text-xs leading-5 text-muted-foreground">
            Captures desktop app and system output. Active app lanes
            appear only when they have data.
          </p>
          <div className="mt-4 max-w-xs space-y-2">
            <div className="flex items-center justify-between gap-3">
              <Label className="text-xs uppercase tracking-wide text-muted-foreground">
                Desktop capture gain
              </Label>
              <span className="text-xs font-medium tabular-nums text-foreground">
                +{config.desktop_audio_gain_db.toFixed(0)} dB
              </span>
            </div>
            <Slider
              aria-label="Desktop capture gain"
              disabled={!config.audio_desktop_enabled}
              max={24}
              min={0}
              onValueChange={([gainDb]) =>
                controller.updateDesktopAudioGainDb(gainDb)
              }
              step={1}
              value={[config.desktop_audio_gain_db]}
            />
            <p className="max-w-[48ch] text-xs leading-4 text-muted-foreground">
              Boosts SOURCE&apos;s copy, not macOS volume.
            </p>
          </div>
        </div>
        <AudioSourceMeter
          enabled={config.audio_desktop_enabled}
          selectedAudioInputId={config.selected_audio_input_id}
          desktopAudioEnabled={config.audio_desktop_enabled}
          desktopGainDb={config.desktop_audio_gain_db}
          source="desktop"
        />
        <Switch
          checked={config.audio_desktop_enabled}
          onCheckedChange={controller.updateAudioDesktopEnabled}
        />
      </div>
    </section>
  );
}

function AudioAnalysisControls({ controller }: { controller: SettingsController }) {
  const { config } = controller;
  if (!config) return null;

  return (
    <section className="rounded-lg border border-border/60 bg-muted/15 p-3">
      <Label className="text-sm">Audio analysis</Label>
      <p className="mt-1 max-w-[52ch] text-xs leading-5 text-muted-foreground">
        Turn off any local analysis that you do not need.
        Waveform and speech activity keep running.
      </p>
      <div className="mt-3 space-y-3">
        <AnalysisToggle
          checked={config.audio_transcription_enabled}
          description="Local speech-to-text with Parakeet."
          label="Transcription"
          onCheckedChange={(enabled) =>
            controller.updateAudioAnalysis("audio_transcription_enabled", enabled)
          }
        />
        <AnalysisToggle
          checked={config.audio_speech_emotion_enabled}
          description="Runs only when speech activity is detected."
          label="Speech emotion"
          onCheckedChange={(enabled) =>
            controller.updateAudioAnalysis("audio_speech_emotion_enabled", enabled)
          }
        />
        <AnalysisToggle
          checked={config.audio_sound_events_enabled}
          description="Detects music and nearby scene sounds."
          label="Sound events"
          onCheckedChange={(enabled) =>
            controller.updateAudioAnalysis("audio_sound_events_enabled", enabled)
          }
        />
      </div>
    </section>
  );
}

function AnalysisToggle({
  checked,
  description,
  label,
  onCheckedChange,
}: {
  checked: boolean;
  description: string;
  label: string;
  onCheckedChange: (enabled: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        <Label className="text-sm">{label}</Label>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
      <Switch checked={checked} onCheckedChange={onCheckedChange} />
    </div>
  );
}
