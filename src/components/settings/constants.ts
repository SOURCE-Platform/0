import { CaptureChannels, Config } from "@/components/settings/types";

export const CHANNEL_META: Array<{
  key: keyof CaptureChannels;
  label: string;
  description: string;
  help: string;
}> = [
  {
    key: "system",
    label: "OS / session events",
    description: "Capture-level system transitions and desktop session events.",
    help: "Records desktop lifecycle events such as capture start/stop, session transitions, app launches/quits, and other operating-system level context SOURCE can detect today.",
  },
  {
    key: "focus",
    label: "Focus and running apps",
    description: "Track the frontmost app and app-set changes over time.",
    help: "Tracks which app SOURCE believes is frontmost, plus snapshots of the current running-app set, so focus time can be separated from background presence.",
  },
  {
    key: "visible_windows",
    label: "Visible windows snapshots",
    description: "Best-effort scene context based on app snapshots.",
    help: "Builds a best-effort view of what was visible on screen at each scene sample. This is not a perfect historical window-server replay, but it helps reconstruct the desktop context around a moment.",
  },
  {
    key: "keyboard",
    label: "Keyboard activity",
    description: "Direct key activity for interactive-time classification.",
    help: "Records keyboard events so SOURCE can tell when you were actively typing rather than only passively viewing content.",
  },
  {
    key: "mouse",
    label: "Mouse activity",
    description: "Movement, clicks, and pointer-driven interaction.",
    help: "Records mouse movement and click activity so SOURCE can classify pointer-driven work separately from typing or passive viewing.",
  },
  {
    key: "ocr",
    label: "OCR text capture",
    description: "Text extraction from retained screen evidence when available.",
    help: "Runs text extraction over retained evidence so OCR Review and PII Review can show what readable text SOURCE captured from the screen.",
  },
  {
    key: "screen_frames",
    label: "Screen keyframes / evidence",
    description: "Retained frames and media anchors for review.",
    help: "Captures evidence frames from the selected display so timeline slices, OCR, and inspector views can point back to visual proof of what was on screen.",
  },
  {
    key: "camera_future",
    label: "Vision / scene",
    description: "Camera-derived posture, motion, and presence states.",
    help: "Uses the local camera plus MediaPipe pose and face/iris analysis to derive presence, posture, motion, and gaze-ready geometry. Attention promotion still depends on an active gaze calibration.",
  },
  {
    key: "audio_future",
    label: "Audio / speech",
    description: "Microphone speech-state capture with optional Whisper ASR.",
    help: "Samples the microphone into local chunks, runs VAD to detect speaking spans, and optionally transcribes finalized speech with the local Whisper install when it is available.",
  },
];

export const RESOURCE_PROFILE_META: Record<
  Config["resource_profile"],
  {
    label: string;
    intervalSeconds: number;
    summary: string;
    details: string;
  }
> = {
  minimal: {
    label: "Minimal",
    intervalSeconds: 15,
    summary: "Lightest scene sampling, good for low-overhead validation.",
    details: "System, focus, and visible-window scene snapshots are sampled every 15 seconds while capture is running.",
  },
  balanced: {
    label: "Balanced",
    intervalSeconds: 5,
    summary: "Default desktop-context cadence for everyday capture.",
    details: "System, focus, and visible-window scene snapshots are sampled every 5 seconds while capture is running.",
  },
  high_fidelity: {
    label: "High Fidelity",
    intervalSeconds: 2,
    summary: "Fastest scene sampling for richer timeline reconstruction.",
    details: "System, focus, and visible-window scene snapshots are sampled every 2 seconds while capture is running.",
  },
};

export const PII_CATEGORIES = [
  { value: "email", label: "Emails" },
  { value: "phone", label: "Phones" },
  { value: "government_id", label: "Government IDs" },
  { value: "credit_card", label: "Credit cards" },
  { value: "ip_address", label: "IP addresses" },
];
