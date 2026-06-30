import { Activity, Eye, FileSearch, Shield } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useDesktopContextWorkspace } from "@/components/desktop-context-workspace/useDesktopContextWorkspace";
import {
  formatDuration,
  healthTone,
  safeFormatDate,
} from "@/components/desktop-context-workspace/utils";

export function TimelineContentTabs({
  controller,
}: {
  controller: ReturnType<typeof useDesktopContextWorkspace>;
}) {
  return (
    <Tabs value={controller.activeView} onValueChange={controller.setActiveView}>
      <TabsList variant="line">
        <TabsTrigger value="timeline" className="gap-2">
          <Activity className="h-4 w-4" />
          Timeline
        </TabsTrigger>
        <TabsTrigger value="apps" className="gap-2">
          <Eye className="h-4 w-4" />
          App Usage
        </TabsTrigger>
        <TabsTrigger value="ocr" className="gap-2">
          <FileSearch className="h-4 w-4" />
          OCR Review
        </TabsTrigger>
        <TabsTrigger value="pii" className="gap-2">
          <Shield className="h-4 w-4" />
          PII Review
        </TabsTrigger>
      </TabsList>

      <TabsContent value="timeline" className="pt-4">
        <Card>
          <CardHeader>
            <CardTitle>Channel Health</CardTitle>
            <CardDescription className="max-w-[58ch]">
              Empty rails should be explainable. These signals show whether each channel is enabled, sampling, and writing new data.
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            {controller.channelStatuses.map((channel) => (
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
      </TabsContent>

      <TabsContent value="apps" className="pt-4">
        <Card>
          <CardHeader>
            <CardTitle>App Usage</CardTitle>
            <CardDescription className="max-w-[58ch]">
              Focused time, visible-on-screen time, and interaction-qualified time stay separate so “app was open” does not equal “I was actively using it.”
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>App</TableHead>
                  <TableHead>Focused</TableHead>
                  <TableHead>Visible</TableHead>
                  <TableHead>Interacted</TableHead>
                  <TableHead>OCR</TableHead>
                  <TableHead className="text-right">Action</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {controller.appUsage?.items.map((item) => (
                  <TableRow key={`${item.appName}-${item.bundleId}`}>
                    <TableCell>
                      <div className="font-medium">{item.appName}</div>
                      <div className="text-xs text-muted-foreground">{item.bundleId || "Bundle unknown"}</div>
                    </TableCell>
                    <TableCell>{formatDuration(item.focusedTimeMs)}</TableCell>
                    <TableCell>{formatDuration(item.visibleTimeMs)}</TableCell>
                    <TableCell>{formatDuration(item.interactionTimeMs)}</TableCell>
                    <TableCell>{item.ocrHitCount}</TableCell>
                    <TableCell className="text-right">
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => {
                          controller.setAppFilter(item.appName);
                          controller.setActiveView("timeline");
                        }}
                      >
                        Filter timeline
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      </TabsContent>

      <TabsContent value="ocr" className="space-y-4 pt-4">
        <Card>
          <CardHeader>
            <CardTitle>OCR Review</CardTitle>
            <CardDescription className="max-w-[58ch]">
              Searchable captured text with app attribution and inline PII badges where detected.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <Input
              placeholder="Search OCR text..."
              value={controller.ocrQuery}
              onChange={(event) => controller.setOcrQuery(event.target.value)}
              onBlur={() => void controller.loadReviewData()}
            />
            <div className="space-y-3">
              {controller.ocrItems.map((item) => (
                <div key={item.id} className="rounded-xl border border-border/70 px-4 py-4">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-medium">{item.appName ?? "Unknown app"}</span>
                    <Badge variant="outline">{safeFormatDate(item.timestamp, "p")}</Badge>
                    <Badge variant="outline">Confidence {Math.round(item.confidence * 100)}%</Badge>
                    {item.piiEntities.map((entity) => (
                      <Badge key={entity.id} variant="secondary">
                        {entity.entityType}
                      </Badge>
                    ))}
                  </div>
                  <p className="mt-3 max-w-[60ch] text-sm leading-6 text-foreground">{item.text}</p>
                </div>
              ))}
              {controller.ocrItems.length === 0 ? (
                <div className="rounded-xl border border-dashed border-border/70 px-4 py-10 text-center text-sm text-muted-foreground">
                  No OCR text matched the current filters.
                </div>
              ) : null}
            </div>
          </CardContent>
        </Card>
      </TabsContent>

      <TabsContent value="pii" className="space-y-4 pt-4">
        <Card>
          <CardHeader>
            <CardTitle>PII Review</CardTitle>
            <CardDescription className="max-w-[58ch]">
              Detect-only by default. This is the user-facing trust surface for what SOURCE recognized as sensitive.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <Select value={controller.piiTypeFilter} onValueChange={controller.setPiiTypeFilter}>
              <SelectTrigger className="w-[240px]">
                <SelectValue placeholder="All entity types" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All entity types</SelectItem>
                <SelectItem value="email">Emails</SelectItem>
                <SelectItem value="phone">Phones</SelectItem>
                <SelectItem value="government_id">Government IDs</SelectItem>
                <SelectItem value="credit_card">Credit cards</SelectItem>
                <SelectItem value="ip_address">IP addresses</SelectItem>
              </SelectContent>
            </Select>

            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Timestamp</TableHead>
                  <TableHead>App</TableHead>
                  <TableHead>Type</TableHead>
                  <TableHead>Preview</TableHead>
                  <TableHead>Confidence</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {controller.piiItems.map((item) => (
                  <TableRow key={item.id}>
                    <TableCell>{safeFormatDate(item.timestamp, "p")}</TableCell>
                    <TableCell>{item.appName ?? "Unknown"}</TableCell>
                    <TableCell>{item.entityType}</TableCell>
                    <TableCell className="font-mono text-xs">{item.redactedPreview}</TableCell>
                    <TableCell>{Math.round(item.confidence * 100)}%</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            {controller.piiItems.length === 0 ? (
              <div className="rounded-xl border border-dashed border-border/70 px-4 py-10 text-center text-sm text-muted-foreground">
                No PII entities matched the current filters.
              </div>
            ) : null}
          </CardContent>
        </Card>
      </TabsContent>
    </Tabs>
  );
}
