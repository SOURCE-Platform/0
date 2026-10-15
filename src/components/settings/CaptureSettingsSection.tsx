import { Circle, StopCircle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Disclosure } from "@/components/ui/page";
import { TooltipProvider } from "@/components/ui/tooltip";
import { CaptureChannelRow } from "./CaptureChannelRow";
import { CHANNEL_META } from "./constants";
import { EvidenceDisplayPicker } from "./EvidenceDisplayPicker";
import { GazeCalibrationCard } from "./GazeCalibrationCard";
import { SettingsController } from "./useSettingsController";

const PRIMARY_CHANNELS = ["audio_future", "screen_frames", "ocr"];
const CHANNELS = [
  ...PRIMARY_CHANNELS.flatMap((key) => CHANNEL_META.filter((channel) => channel.key === key)),
  ...CHANNEL_META.filter((channel) => !PRIMARY_CHANNELS.includes(channel.key)),
];

export function CaptureSettingsSection({ controller }: { controller: SettingsController }) {
  if (!controller.config) return null;
  const active = controller.captureStatus?.isActive;
  const display = controller.displays.find((item) => String(item.id) === controller.selectedDisplay) ?? null;
  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="flex flex-wrap items-start justify-between gap-4">
          <div className="space-y-1.5">
            <CardTitle>Capture sources</CardTitle>
            <CardDescription>Choose the data SOURCE captures on this Mac.</CardDescription>
          </div>
          <div className="flex items-center gap-3">
            <Badge variant="outline">{active ? "Running" : "Stopped"}</Badge>
            <Button size="sm" variant={active ? "outline" : "default"} onClick={active ? controller.handleStopCapture : controller.handleStartCapture}
              disabled={!active && (!controller.selectedDisplay || controller.saving)}>
              {active ? <StopCircle /> : <Circle className="fill-current" />}
              {active ? "Stop capture" : "Start capture"}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-5">
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg bg-muted/30 px-3 py-3">
            <div className="space-y-1">
              <div className="text-xs font-medium">Capture display</div>
              <p className="text-xs text-muted-foreground">{controller.captureStatus?.channelsEnabled.length ?? 0} channels enabled</p>
            </div>
            <EvidenceDisplayPicker controller={controller} />
          </div>
          <TooltipProvider>
            <div className="divide-y divide-border">
              {CHANNELS.map((channel) => <CaptureChannelRow key={channel.key} channel={channel} controller={controller} />)}
            </div>
          </TooltipProvider>
        </CardContent>
      </Card>
      <Disclosure title="Gaze calibration" description="Calibrate camera tracking for the selected display.">
        <GazeCalibrationCard display={display} visionEnabled={controller.config.capture_channels.camera_future} />
      </Disclosure>
    </div>
  );
}
