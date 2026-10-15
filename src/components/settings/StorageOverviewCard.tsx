import { FolderOpen } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Disclosure } from "@/components/ui/page";
import { SettingsController } from "./useSettingsController";
import { formatBytes } from "./utils";

export function StorageOverviewCard({ controller }: { controller: SettingsController }) {
  const data = controller.dataOverview;
  const used = data?.diskTotalBytes ? Math.min(100, data.diskUsedBytes / data.diskTotalBytes * 100) : 0;
  return (
    <Card>
      <CardHeader className="flex flex-wrap items-center justify-between gap-3">
        <CardTitle>Local capture storage</CardTitle>
        <Badge variant={data?.diskHealth === "critical" ? "destructive" : "outline"}>{data?.diskHealth ?? "Loading…"}</Badge>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="grid grid-cols-2 gap-5 md:grid-cols-4">
          <Metric label="Total capture data" value={data ? formatBytes(data.totalSizeBytes) : "—"} />
          <Metric label="Database" value={data ? formatBytes(data.databaseSizeBytes) : "—"} />
          <Metric label="Recordings folder" value={data ? formatBytes(data.recordingsSizeBytes) : "—"} />
          <Metric label="Free disk space" value={data ? formatBytes(data.diskFreeBytes) : "—"} />
        </div>
        <div className="space-y-2">
          <div className="flex flex-wrap justify-between gap-2 text-xs text-muted-foreground">
            <span>{used.toFixed(1)}% of this disk is used</span>
            <span>SOURCE uses {(data?.sourcePercentOfDisk ?? 0).toFixed(2)}% of disk capacity</span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-muted" role="meter" aria-label="Disk space used" aria-valuenow={used} aria-valuemin={0} aria-valuemax={100}>
            <div className={`h-full rounded-full ${data?.diskHealth === "critical" ? "bg-destructive" : "bg-foreground/30"}`} style={{ width: `${used}%` }} />
          </div>
          {data?.diskWarning && <p role="alert" className="text-sm text-destructive">{data.diskWarning}</p>}
        </div>
        <Disclosure title="Files and locations" description="Find the database and capture folders on this Mac." className="bg-transparent">
          <div className="divide-y divide-border">
            <PathRow title="Database" path={data?.databasePath} onReveal={() => controller.handleRevealPath("database")} />
            <PathRow title="Recordings" path={data?.actualRecordingsPath} onReveal={() => controller.handleRevealPath("recordings")} />
            <PathRow title="Configured storage" path={data?.configuredStoragePath ?? controller.config?.storage_path} onReveal={() => controller.handleRevealPath("configured_storage")} />
          </div>
        </Disclosure>
      </CardContent>
    </Card>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return <div className="space-y-1.5"><div className="text-xs text-muted-foreground">{label}</div><div className="text-2xl font-medium tracking-tight tabular-nums">{value}</div></div>;
}

function PathRow({ title, path, onReveal }: { title: string; path?: string; onReveal: () => void }) {
  return <div className="flex items-center justify-between gap-4 py-3 first:pt-0 last:pb-0">
    <div className="min-w-0 space-y-1"><div className="text-sm font-medium">{title}</div><p className="break-all font-mono text-xs text-muted-foreground">{path ?? "Loading…"}</p></div>
    <Button variant="ghost" size="sm" aria-label={`Reveal ${title.toLowerCase()} in Finder`} onClick={onReveal}><FolderOpen />Reveal</Button>
  </div>;
}
