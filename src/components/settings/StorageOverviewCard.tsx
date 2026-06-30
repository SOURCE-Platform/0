import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { SettingsController } from "@/components/settings/useSettingsController";
import { formatBytes } from "@/components/settings/utils";

export function StorageOverviewCard({ controller }: { controller: SettingsController }) {
  const overview = controller.dataOverview;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Capture Data Access</CardTitle>
        <CardDescription className="max-w-[60ch]">
          SOURCE stores most channel data in one local SQLite database, with screen evidence files in a recordings folder beside it.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="rounded-xl border border-border/70 bg-muted/20 px-4 py-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <div className="text-sm font-medium text-foreground">Disk safety</div>
              <p className="mt-1 max-w-[60ch] text-sm text-muted-foreground">
                SOURCE footprint is compared against the remaining free space on this disk.
              </p>
            </div>
            <Badge
              variant={
                overview?.diskHealth === "critical"
                  ? "destructive"
                  : overview?.diskHealth === "warning"
                    ? "outline"
                    : "secondary"
              }
            >
              {overview?.diskHealth ?? "unknown"}
            </Badge>
          </div>

          <div className="mt-4 grid gap-4 md:grid-cols-4">
            <Metric label="SOURCE total" value={formatBytes(overview?.totalSizeBytes ?? 0)} />
            <Metric label="Disk free" value={formatBytes(overview?.diskFreeBytes ?? 0)} />
            <Metric label="Disk capacity" value={formatBytes(overview?.diskTotalBytes ?? 0)} />
            <Metric
              label="SOURCE vs free"
              value={`${(overview?.sourcePercentOfFreeSpace ?? 0).toFixed(1)}%`}
            />
          </div>

          <div className="mt-4 space-y-3">
            <ProgressRow
              label="SOURCE share of whole disk"
              value={`${(overview?.sourcePercentOfDisk ?? 0).toFixed(2)}%`}
              percent={Math.min(overview?.sourcePercentOfDisk ?? 0, 100)}
              tone="bg-blue-500"
            />
            <ProgressRow
              label="Disk already used"
              value={
                overview?.diskTotalBytes
                  ? `${((overview.diskUsedBytes / overview.diskTotalBytes) * 100).toFixed(1)}%`
                  : "0.0%"
              }
              percent={
                overview?.diskTotalBytes
                  ? Math.min((overview.diskUsedBytes / overview.diskTotalBytes) * 100, 100)
                  : 0
              }
              tone={
                overview?.diskHealth === "critical"
                  ? "bg-red-500"
                  : overview?.diskHealth === "warning"
                    ? "bg-amber-500"
                    : "bg-emerald-500"
              }
            />
          </div>

          {overview?.diskWarning ? (
            <div className="mt-4 rounded-lg border border-red-500/30 bg-red-950/30 px-3 py-3">
              <p className="max-w-[60ch] text-sm text-red-100">{overview.diskWarning}</p>
            </div>
          ) : null}
        </div>

        <div className="grid gap-4 md:grid-cols-3">
          <StorageStatCard
            title="Database"
            value={formatBytes(overview?.databaseSizeBytes ?? 0)}
            description="SQLite file holding system, focus, OCR, keyboard, mouse, and review metadata."
          />
          <StorageStatCard
            title="Recordings folder"
            value={formatBytes(overview?.recordingsSizeBytes ?? 0)}
            description="Screen keyframes, base layers, and encoded evidence segments live here."
          />
          <StorageStatCard
            title="Total footprint"
            value={formatBytes(overview?.totalSizeBytes ?? 0)}
            description="Combined local storage currently used by SOURCE capture data."
          />
        </div>

        <PathCard
          title="Database file"
          path={overview?.databasePath ?? "Loading database path..."}
          actionLabel="Reveal in Finder"
          onClick={() => controller.handleRevealPath("database")}
        />
        <PathCard
          title="Runtime recordings path"
          path={overview?.actualRecordingsPath ?? "Loading recordings path..."}
          actionLabel="Reveal in Finder"
          onClick={() => controller.handleRevealPath("recordings")}
        />
        <PathCard
          title="Configured storage path"
          path={overview?.configuredStoragePath ?? controller.config?.storage_path ?? ""}
          actionLabel="Reveal in Finder"
          onClick={() => controller.handleRevealPath("configured_storage")}
        />
      </CardContent>
    </Card>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 text-xl font-semibold text-foreground">{value}</div>
    </div>
  );
}

function ProgressRow({
  label,
  value,
  percent,
  tone,
}: {
  label: string;
  value: string;
  percent: number;
  tone: string;
}) {
  return (
    <div>
      <div className="mb-1 flex items-center justify-between text-xs text-muted-foreground">
        <span>{label}</span>
        <span>{value}</span>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-white/8">
        <div className={`h-full rounded-full ${tone}`} style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}

function StorageStatCard({
  title,
  value,
  description,
}: {
  title: string;
  value: string;
  description: string;
}) {
  return (
    <div className="rounded-xl border border-border/70 bg-muted/20 px-4 py-4">
      <div className="text-sm font-medium text-foreground">{title}</div>
      <div className="mt-2 text-2xl font-semibold text-foreground">{value}</div>
      <p className="mt-2 max-w-[32ch] text-xs text-muted-foreground">{description}</p>
    </div>
  );
}

function PathCard({
  title,
  path,
  actionLabel,
  onClick,
}: {
  title: string;
  path: string;
  actionLabel: string;
  onClick: () => void;
}) {
  return (
    <div className="rounded-xl border border-border/70 px-4 py-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="space-y-1">
          <div className="text-sm font-medium text-foreground">{title}</div>
          <p className="max-w-[60ch] break-all text-xs text-muted-foreground">{path}</p>
        </div>
        <Button type="button" variant="outline" size="sm" onClick={onClick}>
          {actionLabel}
        </Button>
      </div>
    </div>
  );
}
