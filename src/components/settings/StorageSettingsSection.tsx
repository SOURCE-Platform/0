import { FolderOpen, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { StorageOverviewCard } from "@/components/settings/StorageOverviewCard";
import { SettingsController } from "@/components/settings/useSettingsController";
import { formatBytes, formatTimestamp } from "@/components/settings/utils";

export function StorageSettingsSection({ controller }: { controller: SettingsController }) {
  const { config, dataOverview } = controller;
  if (!config) return null;

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle>Storage Location</CardTitle>
          <CardDescription className="max-w-[60ch]">
            This is where SOURCE is currently writing local capture data.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <Input value={config.storage_path} readOnly />
          <Button type="button" variant="outline" className="gap-2" onClick={() => controller.handleRevealPath("configured_storage")}>
            <FolderOpen className="h-4 w-4" />
            Open in Finder
          </Button>
        </CardContent>
      </Card>

      <StorageOverviewCard controller={controller} />
      <ChannelDataFootprintCard controller={controller} />
      <RawCapturePreviewCard controller={controller} />
      <RetentionCard controller={controller} />
      <DeleteAllDataCard controller={controller} />

      {!dataOverview?.notes.length ? null : (
        <Card>
          <CardHeader>
            <CardTitle>What to Expect Today</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {dataOverview.notes.map((note) => (
              <p key={note} className="max-w-[60ch] text-sm text-muted-foreground">
                {note}
              </p>
            ))}
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function ChannelDataFootprintCard({ controller }: { controller: SettingsController }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Channel Data Footprint</CardTitle>
        <CardDescription className="max-w-[60ch]">
          Review what each capture channel has stored so far and clear one channel at a time when needed.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {(controller.dataOverview?.channels ?? []).map((channel) => (
          <div key={channel.channel} className="rounded-xl border border-border/70 px-4 py-4">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div className="space-y-2">
                <div className="text-base font-medium text-foreground">{channel.label}</div>
                <div className="space-y-1 text-xs text-muted-foreground">
                  <div>Storage: {channel.storageKind}</div>
                  <div>Rows stored: {channel.rowCount.toLocaleString()}</div>
                  <div>Last sample: {formatTimestamp(channel.lastEventTime)}</div>
                  {channel.diskBytes > 0 ? <div>File footprint: {formatBytes(channel.diskBytes)}</div> : null}
                </div>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="gap-2"
                onClick={() => controller.handleDeleteData(channel.channel, channel.label)}
                disabled={controller.captureStatus?.isActive || controller.deletingKey !== null}
              >
                <Trash2 className="h-4 w-4" />
                {controller.deletingKey === channel.channel ? "Deleting..." : "Delete channel data"}
              </Button>
            </div>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

function RawCapturePreviewCard({ controller }: { controller: SettingsController }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Raw Capture Preview</CardTitle>
        <CardDescription className="max-w-[60ch]">
          Inspect the actual rows SOURCE is storing so you can see the raw captured structure, not just summary metrics.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="inline-flex w-full overflow-hidden rounded-lg border border-border md:w-auto">
          {(controller.dataOverview?.channels ?? []).map((channel) => {
            const selected = controller.previewChannel === channel.channel;
            return (
              <button
                key={channel.channel}
                type="button"
                onClick={() => controller.setPreviewChannel(channel.channel)}
                className={`px-3 py-2 text-sm transition-colors ${
                  selected
                    ? "bg-white/8 text-foreground ring-1 ring-white/18"
                    : "bg-muted/18 text-muted-foreground hover:bg-muted/28 hover:text-foreground"
                }`}
              >
                {channel.label}
              </button>
            );
          })}
        </div>

        <div className="rounded-xl border border-border/70 bg-muted/20 px-4 py-4">
          {controller.loadingPreview ? (
            <p className="text-sm text-muted-foreground">Loading raw capture preview...</p>
          ) : controller.channelPreview?.rows.length ? (
            <div className="space-y-4">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <div className="font-medium text-foreground">{controller.channelPreview.label}</div>
                  <p className="max-w-[60ch] text-sm text-muted-foreground">
                    These are recent stored rows from the local database for this channel.
                  </p>
                </div>
                <Badge variant="outline">{controller.channelPreview.rows.length} recent rows</Badge>
              </div>

              <div className="space-y-3">
                {controller.channelPreview.rows.map((row, index) => (
                  <details
                    key={`${row.timestamp ?? "none"}-${index}`}
                    className="rounded-lg border border-border/70 bg-background/30 px-3 py-3"
                  >
                    <summary className="list-none">
                      <div className="flex flex-wrap items-start justify-between gap-3">
                        <div className="space-y-1">
                          <div className="text-sm font-medium text-foreground">{row.summary}</div>
                          <p className="text-xs text-muted-foreground">{formatTimestamp(row.timestamp)}</p>
                        </div>
                        <span className="text-xs uppercase tracking-wide text-muted-foreground">View JSON</span>
                      </div>
                    </summary>
                    <pre className="mt-3 overflow-x-auto rounded-lg border border-border/60 bg-black/20 p-3 text-xs leading-5 text-foreground">
{row.rawJson}
                    </pre>
                  </details>
                ))}
              </div>
            </div>
          ) : (
            <p className="max-w-[60ch] text-sm text-muted-foreground">
              No stored rows yet for this channel. Record some data and come back here to inspect the raw payloads.
            </p>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function RetentionCard({ controller }: { controller: SettingsController }) {
  const config = controller.config;
  if (!config) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Retention</CardTitle>
        <CardDescription className="max-w-[60ch]">
          How long different capture artifacts are kept.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 md:grid-cols-2">
        {Object.entries(config.retention_days).map(([key, value]) => (
          <div key={key} className="space-y-2">
            <Label className="text-base capitalize">{key.split("_").join(" ")}</Label>
            <Input
              type="number"
              min="1"
              value={value}
              onChange={(event) =>
                controller.updateConfig({
                  retention_days: {
                    ...config.retention_days,
                    [key]: Number.parseInt(event.target.value, 10) || 1,
                  },
                })
              }
            />
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

function DeleteAllDataCard({ controller }: { controller: SettingsController }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Delete All Capture Data</CardTitle>
        <CardDescription className="max-w-[60ch]">
          Remove every stored capture row and every retained evidence file from this local SOURCE installation.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-wrap items-center justify-between gap-3">
        <p className="max-w-[60ch] text-sm text-muted-foreground">
          Stop capture first, then use this if you want a clean slate across all channels.
        </p>
        <Button
          type="button"
          variant="destructive"
          className="gap-2"
          onClick={() => controller.handleDeleteData(null, "all capture")}
          disabled={controller.captureStatus?.isActive || controller.deletingKey !== null}
        >
          <Trash2 className="h-4 w-4" />
          {controller.deletingKey === "all" ? "Deleting..." : "Delete everything"}
        </Button>
      </CardContent>
    </Card>
  );
}
