import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Camera, Crosshair } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  buildCalibrationSteps,
  CalibrationStep,
  emitToast,
  qualityLabel,
} from "@/components/settings/gazeCalibrationShared";
import { GazeCalibration } from "@/types/gaze";

function getRequiredParam(search: URLSearchParams, key: string) {
  const value = search.get(key);
  if (!value) {
    throw new Error(`Missing calibration parameter: ${key}`);
  }
  return value;
}

function parseOverlayConfig() {
  const search = new URLSearchParams(window.location.search);
  return {
    calibrationId: getRequiredParam(search, "calibrationId"),
    displayId: Number.parseInt(getRequiredParam(search, "displayId"), 10),
    displayName: getRequiredParam(search, "displayName"),
    screenWidth: Number.parseInt(getRequiredParam(search, "screenWidth"), 10),
    screenHeight: Number.parseInt(getRequiredParam(search, "screenHeight"), 10),
  };
}

export function GazeCalibrationOverlayPage() {
  const config = useMemo(() => parseOverlayConfig(), []);
  const steps = useMemo(
    () => buildCalibrationSteps(config.screenWidth, config.screenHeight),
    [config.screenHeight, config.screenWidth],
  );
  const [stepIndex, setStepIndex] = useState(0);
  const [introCountdown, setIntroCountdown] = useState<number | null>(null);
  const [captureState, setCaptureState] = useState<
    "ready" | "moving" | "settling" | "capturing" | "finalizing" | "error"
  >("ready");
  const [errorText, setErrorText] = useState<string | null>(null);
  const runTokenRef = useRef(0);

  const currentStep = steps[stepIndex];

  useEffect(() => {
    window.focus();
    void restartCalibrationPass();
    return () => {
      runTokenRef.current += 1;
    };
  }, []);

  async function closeOverlay() {
    localStorage.setItem("source-gaze-calibration-refresh-at", String(Date.now()));
    await getCurrentWebviewWindow().close();
  }

  async function restartCalibrationPass() {
    const token = runTokenRef.current + 1;
    runTokenRef.current = token;
    setStepIndex(0);
    setErrorText(null);
    setCaptureState("ready");

    for (const value of [3, 2, 1]) {
      if (token !== runTokenRef.current) return;
      setIntroCountdown(value);
      await waitMs(700);
    }
    if (token !== runTokenRef.current) return;
    setIntroCountdown(null);

    for (let index = 0; index < steps.length; index += 1) {
      if (token !== runTokenRef.current) return;
      setStepIndex(index);
      if (index > 0) {
        setCaptureState("moving");
        await waitMs(420);
      }
      if (token !== runTokenRef.current) return;
      setCaptureState("settling");
      await waitMs(180);
      if (token !== runTokenRef.current) return;
      setCaptureState(index >= steps.length - 1 ? "finalizing" : "capturing");
      const succeeded = await handleCaptureStep(
        steps[index],
        index >= steps.length - 1,
      );
      if (!succeeded || index >= steps.length - 1) return;
      await waitMs(260);
    }
  }

  async function handleCaptureStep(
    step: CalibrationStep,
    shouldFinish: boolean,
  ) {
    setErrorText(null);
    try {
      const calibration = await invoke<GazeCalibration>(
        "capture_gaze_calibration_sample",
        {
          calibrationId: config.calibrationId,
          phase: step.phase,
          targetX: step.targetX,
          targetY: step.targetY,
        },
      );

      if (shouldFinish) {
        const finalized = await invoke<GazeCalibration>(
          "finalize_gaze_calibration",
          {
            calibrationId: calibration.calibrationId,
          },
        );

        emitToast({
          type: finalized.active ? "success" : "error",
          text: finalized.active
            ? `${config.displayName} calibration finished with ${qualityLabel(finalized.validationQuality)} quality.`
            : `${config.displayName} calibration finished, but validation was too weak to activate it.`,
        });
        await closeOverlay();
        return true;
      }
    } catch (error) {
      const message = `Failed to capture calibration sample: ${serializeUnknownError(error)}`;
      setErrorText(message);
      setCaptureState("error");
      emitToast({
        type: "error",
        text: message,
      });
      return false;
    }
    return true;
  }

  return (
    <div className="relative h-screen w-screen overflow-hidden bg-black text-white">
      <div className="absolute inset-0 bg-[radial-gradient(circle_at_center,rgba(255,255,255,0.04),transparent_55%)]" />

      <div className="relative flex h-full flex-col px-8 py-7">
        <div className="flex items-start justify-between gap-4">
          <div className="space-y-2">
            <div className="text-sm uppercase tracking-[0.22em] text-white/65">
              Gaze calibration
            </div>
            <h1 className="text-3xl font-semibold tracking-tight">
              {config.displayName}
            </h1>
            <p className="max-w-[56ch] text-sm leading-6 text-white/78">
              Follow the glowing dot with your eyes. SOURCE moves, pauses,
              and captures automatically so the full pass feels continuous.
            </p>
          </div>
          <Badge
            variant="outline"
            className="border-white/20 bg-white/10 text-white"
          >
            {stepIndex + 1} / {steps.length}
          </Badge>
        </div>

        <div className="mt-8 flex-1">
          {currentStep ? (
            <TargetDot
              step={currentStep}
              width={config.screenWidth}
              height={config.screenHeight}
              state={captureState}
            />
          ) : null}
          {introCountdown !== null ? (
            <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
              <div className="rounded-[2rem] border border-white/15 bg-black/55 px-8 py-6 text-center shadow-2xl backdrop-blur-md">
                <div className="text-xs uppercase tracking-[0.22em] text-white/60">
                  Get ready
                </div>
                <div className="mt-3 text-7xl font-semibold tabular-nums">
                  {introCountdown}
                </div>
                <div className="mt-3 max-w-[28ch] text-sm leading-6 text-white/78">
                  Lock onto the glowing dot. The guided sweep begins as soon
                  as the countdown ends.
                </div>
              </div>
            </div>
          ) : null}
        </div>

        <div className="flex items-end justify-between gap-6">
          <div className="space-y-2">
            <div className="text-2xl font-semibold">{currentStep.label}</div>
            {errorText ? (
              <div className="max-w-[60ch] rounded-2xl border border-red-500/45 bg-red-950/70 px-4 py-3 text-sm leading-6 text-red-100">
                {errorText}
              </div>
            ) : null}
            <div className="flex items-center gap-3 rounded-full border border-white/10 bg-black/30 px-4 py-2 text-sm text-white/80">
              <Camera className="h-4 w-4" />
              {statusCopy(introCountdown, captureState)}
            </div>
          </div>

          <div className="flex gap-3">
            <Button type="button" variant="outline" onClick={() => void closeOverlay()}>
              Cancel
            </Button>
            <Button
              type="button"
              onClick={() => void restartCalibrationPass()}
              disabled={introCountdown !== null}
            >
              <Crosshair className="h-4 w-4" />
              {introCountdown !== null
                ? `Starting in ${introCountdown}`
                : "Restart pass"}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

function TargetDot({
  step,
  width,
  height,
  state,
}: {
  step: CalibrationStep;
  width: number;
  height: number;
  state: "ready" | "moving" | "settling" | "capturing" | "finalizing" | "error";
}) {
  const left = `${(step.targetX / Math.max(1, width)) * 100}%`;
  const top = `${(step.targetY / Math.max(1, height)) * 100}%`;
  const scale =
    state === "capturing" || state === "finalizing"
      ? "1.18"
      : state === "settling"
        ? "1.06"
        : "1";
  const glow =
    state === "capturing" || state === "finalizing"
      ? "0 0 64px rgba(250,204,21,0.92)"
      : "0 0 40px rgba(251,191,36,0.7)";
  const background =
    state === "capturing" || state === "finalizing"
      ? "rgb(253 224 71)"
      : "rgb(245 158 11)";

  return (
    <div
      className="pointer-events-none absolute h-10 w-10 -translate-x-1/2 -translate-y-1/2 rounded-full border border-amber-200 bg-amber-500 shadow-[0_0_40px_rgba(251,191,36,0.7)]"
      style={{
        left,
        top,
        background,
        boxShadow: glow,
        transform: `translate(-50%, -50%) scale(${scale})`,
        transition:
          "left 420ms cubic-bezier(0.22, 1, 0.36, 1), top 420ms cubic-bezier(0.22, 1, 0.36, 1), transform 160ms ease, box-shadow 160ms ease, background-color 160ms ease",
      }}
    />
  );
}

function statusCopy(
  introCountdown: number | null,
  captureState: "ready" | "moving" | "settling" | "capturing" | "finalizing" | "error",
) {
  if (introCountdown !== null) {
    return `Calibration sweep starts in ${introCountdown}.`;
  }

  switch (captureState) {
    case "moving":
      return "Track the moving dot. SOURCE is not sampling yet.";
    case "settling":
      return "Hold steady. SOURCE is letting your gaze settle.";
    case "capturing":
      return "Capturing now. Keep your eyes fixed on the dot.";
    case "finalizing":
      return "Final sample captured. SOURCE is validating the display map.";
    case "error":
      return "Calibration paused because a sample failed. Restart the pass after the issue is fixed.";
    default:
      return "SOURCE will run the full pass automatically.";
  }
}

async function waitMs(duration: number) {
  await new Promise((resolve) => window.setTimeout(resolve, duration));
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
