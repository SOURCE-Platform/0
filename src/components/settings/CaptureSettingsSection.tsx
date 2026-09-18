import {
  Activity,
  Circle,
  CircleHelp,
  StopCircle,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { AudioCaptureControls } from "@/components/settings/AudioCaptureControls";
import { OcrCaptureControls } from "@/components/settings/OcrCaptureControls";
import { ScreenPermissionNotice } from "@/components/settings/ScreenPermissionNotice";
import { CHANNEL_META } from "@/components/settings/constants";
import { EvidenceDisplayPicker } from "@/components/settings/EvidenceDisplayPicker";
import { GazeCalibrationCard } from "@/components/settings/GazeCalibrationCard";
import { SettingsController } from "@/components/settings/useSettingsController";
import { formatChannelActivity, formatTimestamp } from "@/components/settings/utils";
export function CaptureSettingsSection({ controller }: { controller: SettingsController }) {
  const { config } = controller;
  if (!config) return null;
  const currentDisplay =
    controller.displays.find((display) => String(display.id) === controller.selectedDisplay) ?? null;
  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>Evidence Display</CardTitle>
          <CardDescription className="max-w-[60ch]">
            The screen SOURCE records while capture is running.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          <EvidenceDisplayPicker controller={controller} />
        </CardContent>
      </Card>
      <Card>
        <CardHeader className="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
          <div className="space-y-1.5">
            <CardTitle>Capture Channels</CardTitle>
            <CardDescription className="max-w-[60ch]">
              When you press Start Capture, SOURCE begins a local session using the enabled channels below and writes data into the local database and recordings folder.
            </CardDescription>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <Badge variant={controller.captureStatus?.isActive ? "default" : "secondary"}>
              {controller.captureStatus?.isActive ? "Running" : "Stopped"}
            </Badge>
            {!controller.captureStatus?.isActive ? (
              <Button
                type="button"
                size="sm"
                className="gap-2"
                onClick={controller.handleStartCapture}
                disabled={!controller.selectedDisplay || controller.saving}
              >
                <Circle className="h-3.5 w-3.5 fill-current" />
                Start Capture
              </Button>
            ) : (
              <Button
                type="button"
                size="sm"
                variant="destructive"
                className="gap-2"
                onClick={controller.handleStopCapture}
              >
                <StopCircle className="h-3.5 w-3.5" />
                Stop Capture
              </Button>
            )}
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-wrap items-center gap-3 rounded-xl border border-border/70 bg-muted/20 px-4 py-3 text-sm text-muted-foreground">
            <Activity className="h-4 w-4" />
            <span>
              {controller.captureStatus?.isActive
                ? controller.captureStatus.displayName
                  ? `Capturing on ${controller.captureStatus.displayName}.`
                  : "Capture is running."
                : "Capture is currently stopped."}
            </span>
            <span>
              {controller.captureStatus?.channelsEnabled.length ?? 0} channels enabled
              {controller.captureStatus?.startedAt
                ? ` · Started ${new Date(controller.captureStatus.startedAt).toLocaleTimeString()}`
                : ""}
            </span>
          </div>
          <TooltipProvider>
            {CHANNEL_META.map((channel) => {
              const status = controller.channelStatuses.find((item) => item.channel === channel.key);
              const isEnabled = config.capture_channels[channel.key];
              const healthLabel = !isEnabled
                ? "off"
                : status?.health === "off"
                  ? "idle"
                  : status?.health ?? "warming_up";

              return (
                <div key={channel.key} className="rounded-xl border border-border/70 px-4 py-4">
                  <div className="flex flex-wrap items-start justify-between gap-6">
                    <div className="min-w-0 flex-1 space-y-2">
                      <div className="flex flex-wrap items-center gap-2">
                        <Label className="text-base">{channel.label}</Label>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <button
                              type="button"
                              className="inline-flex h-5 w-5 items-center justify-center rounded-full text-muted-foreground transition-colors hover:text-foreground"
                              aria-label={`Explain ${channel.label}`}
                            >
                              <CircleHelp className="h-4 w-4" />
                            </button>
                          </TooltipTrigger>
                          <TooltipContent side="top" className="max-w-xs text-left leading-5">
                            {channel.help}
                          </TooltipContent>
                        </Tooltip>
                        {status || !isEnabled ? (
                          <Badge
                            variant={
                              healthLabel === "healthy"
                                ? "default"
                                : healthLabel === "off"
                                  ? "secondary"
                                  : "outline"
                            }
                          >
                            {healthLabel}
                          </Badge>
                        ) : null}
                      </div>
                      <p className="text-sm text-muted-foreground">{channel.description}</p>
                      {status ? (
                        <div className="space-y-1 text-xs text-muted-foreground">
                          <div>
                            Permission: {status.permissionState} · Last sample: {formatTimestamp(status.lastEventTime)}
                          </div>
                          <div>
                            {formatChannelActivity(
                              status,
                              controller.captureStatus?.isActive ?? false,
                              isEnabled,
                            )}
                          </div>
                        </div>
                      ) : null}
                      {channel.key === "audio_future" ? (
                        <AudioCaptureControls controller={controller} />
                      ) : null}
                      {channel.key === "screen_frames" &&
                      status?.permissionState === "system_denied" ? (
                        <ScreenPermissionNotice />
                      ) : null}
                      {channel.key === "ocr" ? (
                        <OcrCaptureControls controller={controller} />
                      ) : null}
                    </div>

                    <div className="flex min-w-[240px] flex-col items-end gap-3">
                      <div className="flex items-center gap-3">
                        <span className="text-xs uppercase tracking-wide text-muted-foreground">
                          {isEnabled ? "On" : "Off"}
                        </span>
                        <Switch
                          checked={isEnabled}
                          disabled={
                            controller.saving ||
                            (channel.key === "ocr" &&
                              !config.capture_channels.screen_frames)
                          }
                          onCheckedChange={(enabled) => controller.updateChannel(channel.key, enabled)}
                        />
                      </div>
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => controller.enableOnlyChannel(channel.key)}
                        disabled={controller.saving}
                      >
                        Solo test this channel
                      </Button>
                    </div>
                  </div>
                  {status?.lastError ? (
                    <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-red-800 dark:bg-red-950/60 dark:text-red-100">
                      Last error: {status.lastError}
                    </div>
                  ) : null}
                </div>
              );
            })}
          </TooltipProvider>
        </CardContent>
      </Card>

      <GazeCalibrationCard
        display={currentDisplay}
        visionEnabled={config.capture_channels.camera_future}
      />
    </div>
  );
}
