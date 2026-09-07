import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { useDesktopContextWorkspace } from "@/components/desktop-context-workspace/useDesktopContextWorkspace";
import { healthTone } from "@/components/desktop-context-workspace/utils";

// Audio-only focus: App Usage, OCR Review, and PII Review tabs were removed;
// their components stay on disk for later source onboarding. Channel Health
// likewise shows only audio channels.
function isAudioChannel(channel: string): boolean {
  return channel.includes("audio");
}

export function TimelineContentTabs({
  controller,
}: {
  controller: ReturnType<typeof useDesktopContextWorkspace>;
}) {
  return (
    <div className="pt-4">
      <Card>
        <CardHeader>
          <CardTitle>Channel Health</CardTitle>
          <CardDescription className="max-w-[58ch]">
            Empty rails should be explainable. These signals show whether the audio channel is
            enabled, sampling, and writing new data.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {controller.channelStatuses.filter((channel) => isAudioChannel(channel.channel)).map((channel) => (
            <div key={channel.channel} className="rounded-xl border border-border/70 px-4 py-3">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <div className="font-medium capitalize">{channel.channel.split("_").join(" ")}</div>
                  <div className="max-w-[28ch] text-xs leading-5 text-muted-foreground">{channel.details}</div>
                </div>
                <Badge variant={healthTone(channel.health)}>{channel.health}</Badge>
              </div>
              <div className="mt-3 flex flex-wrap gap-2 text-xs text-muted-foreground">
                <span>Permission: {channel.permissionState}</span>
                <span>Samples: {channel.sampleCount}</span>
                <span>Rate: {channel.throughputPerMinute.toFixed(2)}/min</span>
              </div>
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
