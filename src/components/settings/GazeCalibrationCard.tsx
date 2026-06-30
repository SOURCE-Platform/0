import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Camera, Crosshair, Eye, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Display } from "@/components/settings/types";
import {
  OBSERVER_APP_TOAST_EVENT,
  ObserverAppToastDetail,
} from "@/lib/app-config-events";
import { GazeCalibration } from "@/types/gaze";

interface CalibrationStep {
  phase: string;
  label: string;
  targetX: number;
  targetY: number;
}

export function GazeCalibrationCard({
  sessionId,
  display,
  visionEnabled,
}: {
  sessionId: string | null;
  display: Display | null;
  visionEnabled: boolean;
}) {
  const [activeCalibration, setActiveCalibration] = useState<GazeCalibration | null>(null);
  const [latestCalibration, setLatestCalibration] = useState<GazeCalibration | null>(null);
  const [draftCalibration, setDraftCalibration] = useState<GazeCalibration | null>(null);
  const [stepIndex, setStepIndex] = useState(0);
  const [overlayOpen, setOverlayOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void refreshCalibration();
  }, []);

  useEffect(() => {
    if (!overlayOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.code !== "Space" || busy) return;
      event.preventDefault();
      void handleCaptureStep();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [overlayOpen, busy, stepIndex, draftCalibration]);

  const screenWidth = display?.width ?? window.innerWidth;
  const screenHeight = display?.height ?? window.innerHeight;
  const steps = buildCalibrationSteps(screenWidth, screenHeight);
  const currentStep = overlayOpen ? steps[stepIndex] : null;
  const visibleCalibration = draftCalibration ?? activeCalibration ?? latestCalibration;

  async function refreshCalibration() {
    try {
      const calibration = await invoke<GazeCalibration | null>("get_active_gaze_calibration");
      setActiveCalibration(calibration);
      if (calibration) setLatestCalibration(calibration);
    } catch (error) {
      emitToast({ type: "error", text: `Failed to load gaze calibration: ${error}` });
    }
  }

  async function handleStartCalibration() {
    setBusy(true);
    try {
      const calibration = await invoke<GazeCalibration>("start_gaze_calibration", {
        sessionId,
        screenWidth,
        screenHeight,
      });
      setDraftCalibration(calibration);
      setLatestCalibration(calibration);
      setStepIndex(0);
      setOverlayOpen(true);
      emitToast({ type: "success", text: "Gaze calibration started." });
    } catch (error) {
      emitToast({ type: "error", text: `Failed to start gaze calibration: ${error}` });
    } finally {
      setBusy(false);
    }
  }

  async function handleCaptureStep() {
    if (!draftCalibration || !currentStep) return;
    setBusy(true);
    try {
      const calibration = await invoke<GazeCalibration>("capture_gaze_calibration_sample", {
        calibrationId: draftCalibration.calibrationId,
        phase: currentStep.phase,
        targetX: currentStep.targetX,
        targetY: currentStep.targetY,
      });
      setDraftCalibration(calibration);
      setLatestCalibration(calibration);
      if (stepIndex >= steps.length - 1) {
        const finalized = await invoke<GazeCalibration>("finalize_gaze_calibration", {
          calibrationId: calibration.calibrationId,
        });
        setDraftCalibration(null);
        setLatestCalibration(finalized);
        setActiveCalibration(finalized.active ? finalized : null);
        setOverlayOpen(false);
        emitToast({
          type: finalized.active ? "success" : "error",
          text: finalized.active
            ? `Gaze calibration finished with ${qualityLabel(finalized.validationQuality)} quality.`
            : "Gaze calibration finished, but validation was too weak to activate it.",
        });
        return;
      }
      setStepIndex((value) => value + 1);
    } catch (error) {
      emitToast({ type: "error", text: `Failed to capture calibration sample: ${error}` });
    } finally {
      setBusy(false);
    }
  }

  function handleCancelCalibration() {
    setDraftCalibration(null);
    setOverlayOpen(false);
    setStepIndex(0);
  }

  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle>Gaze Calibration</CardTitle>
          <CardDescription className="max-w-[60ch]">
            Calibrate webcam-based gaze mapping so SOURCE can connect MediaPipe face and iris geometry to on-screen attention targets.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={activeCalibration ? "default" : "outline"}>
              {activeCalibration ? qualityLabel(activeCalibration.validationQuality) : "Not calibrated"}
            </Badge>
            <Badge variant="outline">
              {display ? `${display.name} · ${display.width}×${display.height}` : "Using current SOURCE window"}
            </Badge>
            {!visionEnabled ? <Badge variant="secondary">Vision channel off</Badge> : null}
          </div>

          <div className="grid gap-3 md:grid-cols-3">
            <InfoTile
              label="Samples"
              value={String(visibleCalibration?.calibrationPoints.length ?? 0)}
              caption="Static points plus sweep checkpoints collected so far."
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
              value={visibleCalibration ? `${visibleCalibration.modelName} ${visibleCalibration.modelVersion}` : "Awaiting run"}
              caption="Current local gaze-estimation stack for this calibration."
            />
          </div>

          <p className="max-w-[60ch] text-sm text-muted-foreground">
            Calibration runs inside SOURCE today. Keep your face centered, both eyes visible, and lighting steady before each sample.
          </p>

          {!activeCalibration ? (
            <p className="max-w-[60ch] text-sm text-muted-foreground">
              Attention blocks can still exist in code, but they only become live capture data after a usable calibration activates.
            </p>
          ) : null}

          <div className="flex flex-wrap items-center gap-3">
            <Button type="button" onClick={handleStartCalibration} disabled={busy}>
              <Eye className="h-4 w-4" />
              Calibrate eye tracking
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

      {overlayOpen && currentStep && draftCalibration ? (
        <div className="fixed inset-0 z-[120] bg-black/82 backdrop-blur-sm">
          <div className="flex h-full flex-col px-6 py-6 text-white">
            <div className="flex items-start justify-between gap-4">
              <div className="space-y-2">
                <div className="text-sm uppercase tracking-[0.22em] text-white/65">Gaze calibration</div>
                <h3 className="text-2xl font-semibold">{currentStep.label}</h3>
                <p className="max-w-[58ch] text-sm leading-6 text-white/78">
                  Follow the glowing dot with your eyes, keep your head fairly still, and press Space to capture and advance.
                </p>
              </div>
              <Badge variant="outline" className="border-white/20 bg-white/10 text-white">
                {stepIndex + 1} / {steps.length}
              </Badge>
            </div>

            <div className="relative mt-6 flex-1 rounded-[2rem] border border-white/10 bg-white/[0.03]">
              <TargetDot step={currentStep} width={screenWidth} height={screenHeight} />
              <div className="absolute bottom-6 left-6 flex items-center gap-3 rounded-full border border-white/10 bg-black/30 px-4 py-2 text-sm text-white/80">
                <Camera className="h-4 w-4" />
                Space captures this point. The button remains here as a backup.
              </div>
              <div className="absolute bottom-6 right-6 flex gap-3">
                <Button type="button" variant="outline" onClick={handleCancelCalibration} disabled={busy}>
                  Cancel
                </Button>
                <Button type="button" onClick={() => void handleCaptureStep()} disabled={busy}>
                  <Crosshair className="h-4 w-4" />
                  {busy ? "Capturing..." : "Capture sample"}
                </Button>
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}

function TargetDot({
  step,
  width,
  height,
}: {
  step: CalibrationStep;
  width: number;
  height: number;
}) {
  const left = `${(step.targetX / Math.max(1, width)) * 100}%`;
  const top = `${(step.targetY / Math.max(1, height)) * 100}%`;
  return (
    <div
      className="absolute h-10 w-10 -translate-x-1/2 -translate-y-1/2 rounded-full border border-amber-200 bg-amber-500 shadow-[0_0_40px_rgba(251,191,36,0.7)]"
      style={{ left, top }}
    />
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

function buildCalibrationSteps(width: number, height: number): CalibrationStep[] {
  const point = (phase: string, label: string, x: number, y: number) => ({
    phase,
    label,
    targetX: Math.round(width * x),
    targetY: Math.round(height * y),
  });
  const grid = [
    point("grid_top_left", "Top left", 0.12, 0.14),
    point("grid_top_center", "Top center", 0.5, 0.14),
    point("grid_top_right", "Top right", 0.88, 0.14),
    point("grid_middle_left", "Middle left", 0.12, 0.5),
    point("grid_center", "Center", 0.5, 0.5),
    point("grid_middle_right", "Middle right", 0.88, 0.5),
    point("grid_bottom_left", "Bottom left", 0.12, 0.86),
    point("grid_bottom_center", "Bottom center", 0.5, 0.86),
    point("grid_bottom_right", "Bottom right", 0.88, 0.86),
  ];
  const sweeps = [
    point("sweep_horizontal_start", "Horizontal sweep", 0.18, 0.5),
    point("sweep_horizontal_mid", "Horizontal sweep", 0.5, 0.5),
    point("sweep_horizontal_end", "Horizontal sweep", 0.82, 0.5),
    point("sweep_vertical_start", "Vertical sweep", 0.5, 0.2),
    point("sweep_vertical_mid", "Vertical sweep", 0.5, 0.5),
    point("sweep_vertical_end", "Vertical sweep", 0.5, 0.8),
    point("sweep_diagonal_start", "Diagonal sweep", 0.2, 0.2),
    point("sweep_diagonal_mid", "Diagonal sweep", 0.5, 0.5),
    point("sweep_diagonal_end", "Diagonal sweep", 0.8, 0.8),
  ];
  return [...grid, ...sweeps];
}

function qualityLabel(quality: string | null | undefined) {
  if (!quality) return "Pending";
  return quality.split("_").join(" ");
}

function emitToast(detail: ObserverAppToastDetail) {
  window.dispatchEvent(new CustomEvent(OBSERVER_APP_TOAST_EVENT, { detail }));
}
