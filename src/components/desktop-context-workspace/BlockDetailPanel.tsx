import { convertFileSrc } from "@tauri-apps/api/core";
import { Layers3, PanelRightOpen, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { OcrReconstructionView } from "@/components/desktop-context-workspace/OcrReconstructionView";
import { useDesktopContextWorkspace } from "@/components/desktop-context-workspace/useDesktopContextWorkspace";
import {
  formatBytes,
  formatDuration,
  safeFormatDate,
} from "@/components/desktop-context-workspace/utils";

export function BlockDetailPanel({
  controller,
}: {
  controller: ReturnType<typeof useDesktopContextWorkspace>;
}) {
  const linkedPath =
    controller.sliceDetail?.linkedFilePaths[0] ?? controller.sliceDetail?.slice.evidenceFramePath ?? null;
  const selectedEvidenceSrc = linkedPath ? convertFileSrc(linkedPath) : null;
  const showVisualTab = !!controller.sliceDetail?.ocrReconstruction || !!selectedEvidenceSrc;
  const showTranscriptTab = controller.sliceDetail?.slice.tags.includes("asr") ?? false;

  return (
    <Card className="h-fit border-border/70 xl:sticky xl:top-20">
      <CardHeader>
        <div className="flex items-center gap-2">
          <PanelRightOpen className="h-4 w-4 text-muted-foreground" />
          <CardTitle>Block Detail</CardTitle>
          {controller.selectedSlice ? (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="ml-auto h-7 w-7 cursor-pointer"
              onClick={() => controller.clearSelectedSlice()}
              aria-label="Close block detail"
            >
              <X className="h-4 w-4" />
            </Button>
          ) : null}
        </div>
        <CardDescription className="max-w-[30ch]">
          Click any timeline block to inspect what it captured, how large it is, and the exact stored payload.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {!controller.selectedSlice ? (
          <div className="rounded-xl border border-dashed border-border/70 px-4 py-12 text-center text-sm text-muted-foreground">
            No timeline block selected yet.
          </div>
        ) : !controller.sliceDetail ? (
          <div className="rounded-xl border border-border/70 px-4 py-12 text-center text-sm text-muted-foreground">
            Loading block detail...
          </div>
        ) : (
          <div className="space-y-4">
            <div className="space-y-2">
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="outline">{controller.sliceDetail.railLabel}</Badge>
                <Badge variant="outline">{controller.sliceDetail.slice.sliceKind}</Badge>
                <Badge variant="outline">{controller.sliceDetail.storageExact ? "Exact" : "Estimated"}</Badge>
                {controller.sliceDetail.slice.piiCount > 0 ? (
                  <Badge variant="secondary">{controller.sliceDetail.slice.piiCount} PII matches</Badge>
                ) : null}
              </div>
              <h3 className="text-lg font-semibold">{controller.sliceDetail.slice.title}</h3>
              <p className="text-sm text-muted-foreground">
                {safeFormatDate(controller.sliceDetail.occurredAt, "PPpp")} • {formatDuration(controller.sliceDetail.durationMs)}
              </p>
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <div className="rounded-xl border border-border/70 bg-muted/20 px-3 py-3">
                <div className="text-xs uppercase tracking-wide text-muted-foreground">Storage</div>
                <div className="mt-1 text-lg font-semibold text-foreground">
                  {formatBytes(controller.sliceDetail.storageBytes)}
                </div>
                <p className="mt-1 text-xs text-muted-foreground">
                  {controller.sliceDetail.storageExact ? "Exact bytes tied to this block." : "Estimated bytes tied to this span."}
                </p>
              </div>
              <div className="rounded-xl border border-border/70 bg-muted/20 px-3 py-3">
                <div className="text-xs uppercase tracking-wide text-muted-foreground">Rows / Files</div>
                <div className="mt-1 text-lg font-semibold text-foreground">
                  {controller.sliceDetail.rowCount} rows • {controller.sliceDetail.fileCount} files
                </div>
                <p className="mt-1 text-xs text-muted-foreground">
                  Source-backed storage accounting for this block.
                </p>
              </div>
            </div>

            <Tabs value={controller.detailTab} onValueChange={controller.setDetailTab}>
              <TabsList variant="line">
                {showTranscriptTab ? <TabsTrigger value="transcript">Transcript</TabsTrigger> : null}
                {showVisualTab ? <TabsTrigger value="visual">Visual</TabsTrigger> : null}
                <TabsTrigger value="metadata">Metadata</TabsTrigger>
                <TabsTrigger value="json">Raw JSON</TabsTrigger>
              </TabsList>

              {showTranscriptTab ? (
                <TabsContent value="transcript" className="pt-4">
                  <div className="rounded-xl border border-border/70 bg-muted/20 p-4">
                    <div className="mb-3 flex items-center gap-2 text-xs uppercase tracking-wide text-muted-foreground">
                      <span className="relative flex h-2 w-2">
                        {!controller.sliceDetail.slice.tags.includes("final") ? (
                          <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-60" />
                        ) : null}
                        <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-400" />
                      </span>
                      {controller.sliceDetail.slice.tags.includes("final") ? "Final transcript" : "Live transcript"}
                    </div>
                    <p className="max-w-[60ch] whitespace-pre-wrap text-base leading-7 text-foreground">
                      {controller.sliceDetail.slice.ocrPreview?.trim() || "Listening for words…"}
                    </p>
                  </div>
                </TabsContent>
              ) : null}

              {showVisualTab ? (
                <TabsContent value="visual" className="space-y-4 pt-4">
                  {controller.sliceDetail.ocrReconstruction ? (
                    <OcrReconstructionView reconstruction={controller.sliceDetail.ocrReconstruction} />
                  ) : selectedEvidenceSrc ? (
                    <div className="space-y-3">
                      <div className="text-xs uppercase tracking-wide text-muted-foreground">Linked evidence frame</div>
                      <img
                        src={selectedEvidenceSrc}
                        alt="Evidence frame"
                        className="w-full rounded-xl border border-border/70 object-cover"
                      />
                    </div>
                  ) : (
                    <div className="rounded-xl border border-dashed border-border/70 px-4 py-10 text-center text-sm text-muted-foreground">
                      No visual reconstruction is available for this block.
                    </div>
                  )}
                </TabsContent>
              ) : null}

              <TabsContent value="metadata" className="space-y-4 pt-4">
                <div className="space-y-3">
                  <MetadataRow label="Focused app" value={controller.sliceDetail.focusedApp ?? "Unknown"} />
                  <MetadataRow label="Source" value={controller.sliceDetail.slice.source} />

                  <div>
                    <div className="flex items-center justify-between gap-2">
                      <div className="text-xs uppercase tracking-wide text-muted-foreground">Visible windows</div>
                      <Badge variant="outline" className="gap-1 text-[10px] uppercase tracking-wide">
                        <Layers3 className="h-3 w-3" />
                        {controller.sliceDetail.visibleWindows.length}
                      </Badge>
                    </div>
                    {controller.sliceDetail.visibleWindows.length === 0 ? (
                      <p className="mt-1 text-sm text-muted-foreground">
                        No visible-window snapshot was available for this moment.
                      </p>
                    ) : (
                      <div className="mt-2 max-h-72 space-y-2 overflow-y-auto pr-1">
                        {controller.sliceDetail.visibleWindows.map((window) => (
                          <div
                            key={`${window.bundleId}-${window.processId}`}
                            className="rounded-lg border border-border/60 bg-muted/25 px-3 py-2 text-sm"
                          >
                            <div className="flex items-start justify-between gap-2">
                              <div className="min-w-0">
                                <div className="truncate font-medium">{window.appName}</div>
                                <div className="truncate text-[11px] text-muted-foreground">
                                  {window.bundleId || "Bundle unknown"}
                                </div>
                              </div>
                              <Badge
                                variant={window.isFrontmost ? "default" : "outline"}
                                className="shrink-0 text-[10px] uppercase tracking-wide"
                              >
                                {window.isFrontmost ? "Frontmost" : `${Math.round(window.confidence * 100)}%`}
                              </Badge>
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>

                  <div>
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">Reasons</div>
                    <ul className="mt-2 space-y-1 text-sm text-foreground">
                      {controller.sliceDetail.interactionReasons.map((reason) => (
                        <li key={reason}>{reason}</li>
                      ))}
                    </ul>
                  </div>

                  {controller.sliceDetail.piiEntities.length > 0 ? (
                    <div>
                      <div className="text-xs uppercase tracking-wide text-muted-foreground">Detected PII</div>
                      <div className="mt-2 space-y-2">
                        {controller.sliceDetail.piiEntities.map((entity) => (
                          <div key={entity.id} className="rounded-lg bg-muted/35 px-3 py-2 text-sm">
                            <div className="font-medium">
                              {entity.entityType} • {entity.redactedPreview}
                            </div>
                            <div className="text-xs text-muted-foreground">
                              Confidence {Math.round(entity.confidence * 100)}%
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}

                  {controller.sliceDetail.linkedFilePaths.length > 0 ? (
                    <div>
                      <div className="text-xs uppercase tracking-wide text-muted-foreground">Linked files</div>
                      <div className="mt-2 space-y-2">
                        {controller.sliceDetail.linkedFilePaths.map((path) => (
                          <p
                            key={path}
                            className="break-all rounded-lg bg-muted/35 px-3 py-2 font-mono text-xs text-foreground"
                          >
                            {path}
                          </p>
                        ))}
                      </div>
                    </div>
                  ) : null}
                </div>
              </TabsContent>

              <TabsContent value="json" className="space-y-4 pt-4">
                {controller.sliceDetail.rawPayloads.map((payload) => (
                  <div key={payload.label} className="space-y-2">
                    <div className="text-xs uppercase tracking-wide text-muted-foreground">{payload.label}</div>
                    <pre className="overflow-x-auto rounded-xl border border-border/70 bg-black/20 p-4 text-xs leading-6 text-foreground">
{payload.rawJson}
                    </pre>
                  </div>
                ))}
              </TabsContent>
            </Tabs>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function MetadataRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 text-sm text-foreground">{value}</div>
    </div>
  );
}
