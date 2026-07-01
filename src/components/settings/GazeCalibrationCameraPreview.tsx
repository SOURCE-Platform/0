import { useEffect, useRef, useState } from "react";

type PreviewState = "idle" | "loading" | "ready" | "error";

export function GazeCalibrationCameraPreview({
  cameraName,
}: {
  cameraName: string | null;
}) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [state, setState] = useState<PreviewState>(
    cameraName ? "loading" : "idle",
  );
  const [errorText, setErrorText] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let activeStream: MediaStream | null = null;

    async function startPreview() {
      if (!cameraName) {
        setState("idle");
        setErrorText(null);
        return;
      }

      if (!navigator.mediaDevices?.getUserMedia) {
        setState("error");
        setErrorText("Live camera preview is unavailable in this environment.");
        return;
      }

      setState("loading");
      setErrorText(null);

      try {
        const primingStream = await navigator.mediaDevices.getUserMedia({
          video: true,
          audio: false,
        });

        const devices = await navigator.mediaDevices.enumerateDevices();
        const matchedDevice = devices.find((device) => {
          return (
            device.kind === "videoinput" &&
            normalizeCameraLabel(device.label).includes(
              normalizeCameraLabel(cameraName),
            )
          );
        });

        let nextStream = primingStream;
        if (matchedDevice?.deviceId) {
          primingStream.getTracks().forEach((track) => track.stop());
          nextStream = await navigator.mediaDevices.getUserMedia({
            video: {
              deviceId: {
                exact: matchedDevice.deviceId,
              },
            },
            audio: false,
          });
        }

        if (cancelled) {
          nextStream.getTracks().forEach((track) => track.stop());
          return;
        }

        activeStream = nextStream;
        const video = videoRef.current;
        if (!video) {
          nextStream.getTracks().forEach((track) => track.stop());
          return;
        }

        video.srcObject = nextStream;
        await video.play().catch(() => undefined);
        if (!cancelled) {
          setState("ready");
        }
      } catch (error) {
        if (!cancelled) {
          setState("error");
          setErrorText(
            error instanceof Error
              ? error.message
              : "SOURCE could not open the selected camera preview.",
          );
        }
      }
    }

    void startPreview();

    return () => {
      cancelled = true;
      if (videoRef.current) {
        videoRef.current.srcObject = null;
      }
      activeStream?.getTracks().forEach((track) => track.stop());
    };
  }, [cameraName]);

  if (!cameraName) {
    return (
      <div className="rounded-2xl border border-dashed border-border/70 bg-muted/10 px-4 py-6 text-sm text-muted-foreground">
        Select a camera to see its live framing preview here.
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="overflow-hidden rounded-2xl border border-border/70 bg-black/30">
        <div className="aspect-video w-full bg-black">
          <video
            ref={videoRef}
            className="h-full w-full object-cover"
            autoPlay
            muted
            playsInline
          />
        </div>
      </div>
      <div className="max-w-[46ch] text-xs leading-5 text-muted-foreground">
        {state === "loading"
          ? "Opening the selected camera so you can confirm framing before calibration starts."
          : state === "ready"
            ? "Live preview of the selected calibration camera."
            : state === "error"
              ? errorText ?? "SOURCE could not open the selected camera preview."
              : "Select a camera to see its framing."}
      </div>
    </div>
  );
}

function normalizeCameraLabel(value: string) {
  return value.toLowerCase().replace(/\s+/g, " ").trim();
}
