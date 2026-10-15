import { CircleHelp } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/page";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { AudioCaptureControls } from "./AudioCaptureControls";
import { OcrCaptureControls } from "./OcrCaptureControls";
import { ScreenPermissionNotice } from "./ScreenPermissionNotice";
import { CHANNEL_META } from "./constants";
import { SettingsController } from "./useSettingsController";
import { formatChannelActivity, formatTimestamp } from "./utils";

export function CaptureChannelRow({ channel, controller }: {
  channel: typeof CHANNEL_META[number];
  controller: SettingsController;
}) {
  const config = controller.config;
  if (!config) return null;
  const status = controller.channelStatuses.find((item) => item.channel === channel.key);
  const enabled = config.capture_channels[channel.key];
  const health = !enabled ? "off" : status?.health === "off" ? "idle" : status?.health ?? "warming_up";
  const id = `capture-${channel.key}`;

  return (
    <div className="py-4 first:pt-0 last:pb-0">
      <div className="flex items-start justify-between gap-5">
        <div className="min-w-0 space-y-1.5">
          <div className="flex flex-wrap items-center gap-2">
            <label htmlFor={id} className="text-sm font-medium">{channel.label}</label>
            <Tooltip>
              <TooltipTrigger asChild>
                <button type="button" aria-label={`Explain ${channel.label}`} className="rounded-full text-muted-foreground hover:text-foreground">
                  <CircleHelp className="h-3.5 w-3.5" />
                </button>
              </TooltipTrigger>
              <TooltipContent className="max-w-xs leading-5">{channel.help}</TooltipContent>
            </Tooltip>
            <Badge variant="outline" className="text-[10px] font-normal">
              {health === "healthy" && <span className="mr-1 h-1.5 w-1.5 rounded-full bg-emerald-500" />}
              {health.split("_").join(" ")}
            </Badge>
          </div>
          <p className="text-sm leading-6 text-muted-foreground">{channel.description}</p>
        </div>
        <Switch id={id} checked={enabled} disabled={controller.saving || (channel.key === "ocr" && !config.capture_channels.screen_frames)}
          onCheckedChange={(value) => controller.updateChannel(channel.key, value)} />
      </div>
      {channel.key === "screen_frames" && status?.permissionState === "system_denied" && <div className="mt-3"><ScreenPermissionNotice /></div>}
      {status?.lastError && <p role="alert" className="mt-3 text-xs text-destructive">{status.lastError}</p>}
      <Disclosure title={channel.key === "audio_future" || channel.key === "ocr" ? "Settings and diagnostics" : "Diagnostics"}
        className="mt-2 border-0 p-0 [&>summary]:w-fit [&>summary]:gap-2 [&>summary]:text-muted-foreground [&>summary_span]:text-xs">
        <div className="space-y-3">
          {status && <div className="space-y-1 text-xs leading-5 text-muted-foreground">
            <p>Permission: {status.permissionState} · Last sample: {formatTimestamp(status.lastEventTime)}</p>
            <p>{formatChannelActivity(status, controller.captureStatus?.isActive ?? false, enabled)}</p>
          </div>}
          {channel.key === "audio_future" && <AudioCaptureControls controller={controller} />}
          {channel.key === "ocr" && <OcrCaptureControls controller={controller} />}
          <Button variant="outline" size="sm" onClick={() => controller.enableOnlyChannel(channel.key)} disabled={controller.saving}>Solo test this channel</Button>
        </div>
      </Disclosure>
    </div>
  );
}
