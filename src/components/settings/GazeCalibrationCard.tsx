import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  WebviewWindow,
  getAllWebviewWindows,
} from "@tauri-apps/api/webviewWindow";
import { CircleHelp, Eye, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { Display } from "@/components/settings/types";
import { GazeCalibrationCameraPreview } from "@/components/settings/GazeCalibrationCameraPreview";
import { emitToast, qualityLabel } from "@/components/settings/gazeCalibrationShared";
import { GazeCalibration, GazeCameraSource } from "@/types/gaze";

export function GazeCalibrationCard({
  display,
  visionEnabled,
}: {
  display: Display | null;
  visionEnabled: boolean;
}) {
  const [activeCalibration, setActiveCalibration] = useState<GazeCalibration | null>(null);
  const [latestCalibration, setLatestCalibration] = useState<GazeCalibration | null>(null);
  const [cameraSources, setCameraSources] = useState<GazeCameraSource[]>([]);
  const [selectedCameraId, setSelectedCameraId] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const displayId = useMemo(
    () => display?.id ?? null,
    [display],
  );

  useEffect(() => {
    void refreshCalibration();
    if (visionEnabled) {
      void refreshCameraSources();
      return;
    }
    setCameraSources([]);
    setSelectedCameraId("");
  }, [displayId, visionEnabled]);

  useEffect(() => {
    const refresh = () => {
      void refreshCalibration();
      if (visionEnabled) {
        void refreshCameraSources();
      }
    };
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [displayId, visionEnabled]);

  useEffect(() => {
    if (!cameraSources.length) {
      setSelectedCameraId("");
      return;
    }

    if (
      latestCalibration?.cameraId &&
      cameraSources.some((source) => source.cameraId === latestCalibration.cameraId)
    ) {
      setSelectedCameraId(latestCalibration.cameraId);
      return;
    }

    setSelectedCameraId((current) => {
      if (current && cameraSources.some((source) => source.cameraId === current)) {
        return current;
      }
      return cameraSources[0]?.cameraId ?? "";
    });
  }, [cameraSources, latestCalibration?.cameraId]);

  const visibleCalibration = activeCalibration ?? latestCalibration;
  const selectedCamera =
    cameraSources.find((source) => source.cameraId === selectedCameraId) ?? null;

  async function refreshCalibration() {
    try {
      const calibration = await invoke<GazeCalibration | null>(
        "get_active_gaze_calibration",
        { displayId },
      );
      setActiveCalibration(calibration);
      setLatestCalibration(calibration);
    } catch (error) {
      emitToast({ type: "error", text: `Failed to load gaze calibration: ${error}` });
    }
  }

  async function refreshCameraSources() {
    try {
      const sources = await invoke<GazeCameraSource[]>("list_gaze_camera_sources");
      setCameraSources(sources);
    } catch (error) {
      emitToast({
        type: "error",
        text: `Failed to inspect camera sources: ${serializeUnknownError(error)}`,
      });
    }
  }

  async function handleStartCalibration() {
    if (!display) {
      emitToast({ type: "error", text: "Choose a display before starting gaze calibration." });
      return;
    }
    if (!selectedCameraId) {
      emitToast({
        type: "error",
        text: "No local camera is available for gaze calibration right now.",
      });
      return;
    }
    setBusy(true);
    try {
      const calibration = await invoke<GazeCalibration>("start_gaze_calibration", {
        displayId: display.id,
        displayName: display.name,
        cameraId: selectedCameraId,
        displayX: display.x,
        displayY: display.y,
        screenWidth: display.width,
        screenHeight: display.height,
      });
      setLatestCalibration(calibration);
      await openCalibrationWindow(display, calibration);
      emitToast({ type: "success", text: `Gaze calibration started on ${display.name}.` });
    } catch (error) {
      emitToast({
        type: "error",
        text: `Failed to start gaze calibration: ${serializeUnknownError(error)}`,
      });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Gaze Calibration</CardTitle>
        <CardDescription className="max-w-[60ch]">
          Calibrate webcam-based gaze mapping per display so SOURCE can
          connect face and iris geometry to the correct screen.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant={activeCalibration ? "default" : "outline"}>
            {activeCalibration
              ? qualityLabel(activeCalibration.validationQuality)
              : "Not calibrated"}
          </Badge>
          <Badge variant="outline">
            {display
              ? `${display.name} · ${display.width}×${display.height}`
              : "No display selected"}
          </Badge>
          {!visionEnabled ? <Badge variant="secondary">Vision channel off</Badge> : null}
        </div>

        <div className="grid gap-3 md:grid-cols-3">
          <InfoTile
            label="Samples"
            value={String(visibleCalibration?.calibrationPoints.length ?? 0)}
            caption="Static points plus sweep checkpoints for this display."
          />
          <InfoTile
            label="Validation"
            value={
              visibleCalibration?.validationErrorPx != null
                ? `${Math.round(visibleCalibration.validationErrorPx)} px`
                : "Pending"
            }
            caption="Average pixel error after held-out validation points."
          />
          <InfoTile
            label="Model"
            value={
              visibleCalibration
                ? `${visibleCalibration.modelName} ${visibleCalibration.modelVersion}`
                : "Awaiting run"
            }
            caption="Current local gaze-estimation stack for this display."
          />
        </div>

        <div className="space-y-3">
          <div className="flex items-center gap-2">
            <Label className="text-sm">Calibration Camera</Label>
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    className="rounded-full text-muted-foreground transition hover:text-foreground"
                    aria-label="How calibration camera preview works"
                  >
                    <CircleHelp className="h-4 w-4" />
                  </button>
                </TooltipTrigger>
                <TooltipContent side="right" className="max-w-[36ch] text-left leading-6">
                  Calibration launches on the selected display instead of
                  staying trapped in the SOURCE window. Attention for this
                  display only becomes active after a usable calibration
                  finishes here.
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          </div>
          {visionEnabled ? (
            <div className="max-w-md space-y-3">
              <Select
                value={selectedCameraId}
                onValueChange={setSelectedCameraId}
                disabled={!cameraSources.length || busy}
              >
                <SelectTrigger>
                  <SelectValue
                    placeholder={
                      cameraSources.length
                        ? "Choose a camera"
                        : "No camera currently detected"
                    }
                  />
                </SelectTrigger>
                <SelectContent>
                  {cameraSources.map((source) => (
                    <SelectItem key={source.cameraId} value={source.cameraId}>
                      {source.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <GazeCalibrationCameraPreview cameraName={selectedCamera?.name ?? null} />
            </div>
          ) : (
            <p className="max-w-[52ch] text-sm text-muted-foreground">
              Turn on Vision / scene before SOURCE checks cameras or opens a live preview.
            </p>
          )}
        </div>

        {visionEnabled && !cameraSources.length ? (
          <p className="max-w-[60ch] text-sm text-amber-200">
            No camera source is currently available. If your MacBook lid is
            closed, connect or enable an external camera such as iPhone,
            Facecam, or Camo before starting calibration.
          </p>
        ) : null}

        <div className="flex flex-wrap items-center gap-3">
          <Button
            type="button"
            onClick={handleStartCalibration}
            disabled={busy || !visionEnabled || !display || !selectedCameraId}
          >
            <Eye className="h-4 w-4" />
            Calibrate selected display
          </Button>
          <Button type="button" variant="outline" onClick={() => void refreshCalibration()} disabled={busy}>
            <RefreshCw className="h-4 w-4" />
            Refresh state
          </Button>
          {!visionEnabled ? (
            <span className="max-w-[44ch] text-xs text-muted-foreground">
              Turn on `Vision / scene` if you want gaze and attention samples during live capture.
            </span>
          ) : null}
        </div>
      </CardContent>
    </Card>
  );
}

function InfoTile({ label, value, caption }: { label: string; value: string; caption: string }) {
  return (
    <div className="rounded-xl border border-border/70 bg-muted/18 px-4 py-3">
      <div className="text-xs uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 text-lg font-semibold text-foreground">{value}</div>
      <p className="mt-1 max-w-[28ch] text-xs leading-5 text-muted-foreground">{caption}</p>
    </div>
  );
}

function serializeUnknownError(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object") {
    try {
      return JSON.stringify(error);
    } catch {
      return String(error);
    }
  }
  return String(error);
}

async function openCalibrationWindow(
  display: Display,
  calibration: GazeCalibration,
) {
  const label = "gaze-calibration";
  const existing = (await getAllWebviewWindows()).find(
    (windowItem) => windowItem.label === label,
  );
  if (existing) {
    await existing.close();
  }

  const url = new URL(window.location.href);
  url.searchParams.set("mode", "gaze-calibration");
  url.searchParams.set("calibrationId", calibration.calibrationId);
  url.searchParams.set("displayId", String(display.id));
  url.searchParams.set("displayName", display.name);
  url.searchParams.set("screenWidth", String(display.width));
  url.searchParams.set("screenHeight", String(display.height));

  const calibrationWindow = new WebviewWindow(label, {
    url: url.toString(),
    title: `SOURCE Calibration · ${display.name}`,
    x: display.x,
    y: display.y,
    width: display.width,
    height: display.height,
    focus: true,
    fullscreen: true,
    resizable: false,
    decorations: false,
    alwaysOnTop: true,
    skipTaskbar: true,
  });

  await new Promise<void>((resolve, reject) => {
    calibrationWindow.once("tauri://created", () => resolve());
    calibrationWindow.once("tauri://error", (error) => {
      reject(new Error(serializeUnknownError(error)));
    });
  });
}
