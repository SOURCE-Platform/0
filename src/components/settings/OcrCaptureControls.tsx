import { SettingsController } from "@/components/settings/useSettingsController";
import {
  OCR_INTERVAL_PRESETS,
  OCR_LANGUAGES,
} from "@/components/settings/ocrConfig";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

export function OcrCaptureControls({
  controller,
}: {
  controller: SettingsController;
}) {
  const { config } = controller;
  if (!config) return null;

  const keyframesOn = config.capture_channels.screen_frames;
  const ocrOn = config.ocr_enabled && config.capture_channels.ocr;
  const interval = config.ocr_interval_seconds ?? 60;
  const languages = config.ocr_languages ?? ["eng"];

  return (
    <div className="max-w-xl space-y-4 pt-2">
      {!keyframesOn ? (
        <p className="max-w-[52ch] rounded-lg border border-amber-400/30 bg-amber-400/[0.07] p-3 text-xs leading-5 text-muted-foreground">
          OCR reads screenshots, so it needs Screen keyframes turned on
          first. Enable that channel above, then come back here.
        </p>
      ) : null}

      <section className="rounded-lg border border-border/60 bg-muted/15 p-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <Label className="text-sm">Read text from screenshots</Label>
            <p className="max-w-[52ch] text-xs leading-5 text-muted-foreground">
              Runs local Tesseract over keyframes on app switches, scene
              changes, and settled scrolling. Takes effect after restarting
              SOURCE.
            </p>
          </div>
          <Switch
            checked={ocrOn}
            disabled={!keyframesOn || controller.saving}
            onCheckedChange={controller.updateOcrEnabled}
          />
        </div>

        <div className="mt-3 space-y-2">
          <Label className="text-xs uppercase tracking-wide text-muted-foreground">
            Backstop interval
          </Label>
          <div className="flex flex-wrap gap-2">
            {OCR_INTERVAL_PRESETS.map((seconds) => (
              <Button
                key={seconds}
                type="button"
                size="sm"
                variant={interval === seconds ? "secondary" : "outline"}
                disabled={!ocrOn}
                onClick={() => controller.updateOcrInterval(seconds)}
              >
                {seconds >= 60 ? `${seconds / 60}m` : `${seconds}s`}
              </Button>
            ))}
          </div>
        </div>

        <div className="mt-3 space-y-2">
          <Label className="text-xs uppercase tracking-wide text-muted-foreground">
            Languages
          </Label>
          <div className="flex flex-wrap gap-2">
            {OCR_LANGUAGES.map((language) => {
              const active = languages.includes(language.code);
              return (
                <Button
                  key={language.code}
                  type="button"
                  size="sm"
                  variant={active ? "secondary" : "outline"}
                  disabled={!ocrOn}
                  onClick={() => controller.toggleOcrLanguage(language.code)}
                  className={cn(active && "border-green-500/50")}
                >
                  {language.label}
                </Button>
              );
            })}
          </div>
          <p className="text-xs text-muted-foreground">
            At least one language stays on. Each needs its data file or
            OCR refuses to start.
          </p>
        </div>
      </section>
    </div>
  );
}
