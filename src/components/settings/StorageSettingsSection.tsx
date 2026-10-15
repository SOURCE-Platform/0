import { Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Disclosure } from "@/components/ui/page";
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
      <StorageOverviewCard controller={controller} />
      <ChannelDataFootprintCard controller={controller} />
      <Disclosure title="Inspect stored data" description="Preview recent database rows for each capture channel.">
        <RawCapturePreviewCard controller={controller} />
      </Disclosure>
      <RetentionCard controller={controller} />
      <DeleteAllDataCard controller={controller} />

      {!dataOverview?.notes.length ? null : (
        <Card>
          <CardHeader>
            <CardTitle>Storage notes</CardTitle>
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
        <CardTitle>Data by source</CardTitle>
        <CardDescription className="max-w-[60ch]">
          Stored records and file sizes for each capture source.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {(controller.dataOverview?.channels ?? []).map((channel) => (
          <div key={channel.channel} className="border-b border-border py-3 first:pt-0 last:border-0 last:pb-0">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div className="space-y-2">
                <div className="text-sm font-medium text-foreground">{channel.label}</div>
                <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
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
                {controller.deletingKey === channel.channel ? "Deleting..." : "Clear data"}
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
    <div>
      <div className="space-y-4">
        <div className="flex w-full flex-wrap rounded-lg border border-border md:w-auto">
          {(controller.dataOverview?.channels ?? []).map((channel) => {
            const selected = controller.previewChannel === channel.channel;
            return (
              <button
                key={channel.channel}
                type="button"
                onClick={() => controller.setPreviewChannel(channel.channel)}
                className={`px-3 py-2 text-sm transition-colors ${
                  selected
                    ? "bg-accent text-foreground"
                    : "text-muted-foreground hover:bg-accent hover:text-foreground"
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
      </div>
    </div>
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
          Days to keep each type of capture data.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 md:grid-cols-2">
        {Object.entries(config.retention_days).map(([key, value]) => (
          <div key={key} className="flex items-center justify-between gap-4 rounded-lg bg-muted/20 px-3 py-2">
            <Label className="text-sm capitalize">{key.split("_").join(" ")}</Label>
            <Input
              className="w-24"
              aria-label={`${key.split("_").join(" ")} retention in days`}
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
