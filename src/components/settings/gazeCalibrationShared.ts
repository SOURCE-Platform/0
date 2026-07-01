import {
  OBSERVER_APP_TOAST_EVENT,
  ObserverAppToastDetail,
} from "@/lib/app-config-events";

export interface CalibrationStep {
  phase: string;
  label: string;
  targetX: number;
  targetY: number;
}

export function buildCalibrationSteps(
  width: number,
  height: number,
): CalibrationStep[] {
  const point = (phase: string, label: string, x: number, y: number) => ({
    phase,
    label,
    targetX: Math.round(width * x),
    targetY: Math.round(height * y),
  });

  return [
    point("snake_top_left", "Upper left", 0.12, 0.14),
    point("snake_top_mid_left", "Upper row", 0.32, 0.14),
    point("snake_top_center", "Upper center", 0.5, 0.14),
    point("snake_top_mid_right", "Upper row", 0.68, 0.14),
    point("snake_top_right", "Upper right", 0.88, 0.14),
    point("snake_upper_right", "Upper right", 0.88, 0.32),
    point("snake_upper_mid_right", "Upper band", 0.68, 0.32),
    point("snake_upper_center", "Upper band", 0.5, 0.32),
    point("snake_upper_mid_left", "Upper band", 0.32, 0.32),
    point("snake_upper_left", "Upper left", 0.12, 0.32),
    point("snake_mid_left", "Middle left", 0.12, 0.5),
    point("snake_mid_mid_left", "Middle band", 0.32, 0.5),
    point("snake_center", "Center", 0.5, 0.5),
    point("snake_mid_mid_right", "Middle band", 0.68, 0.5),
    point("snake_mid_right", "Middle right", 0.88, 0.5),
    point("snake_lower_right", "Lower right", 0.88, 0.68),
    point("snake_lower_mid_right", "Lower band", 0.68, 0.68),
    point("snake_lower_center", "Lower band", 0.5, 0.68),
    point("snake_lower_mid_left", "Lower band", 0.32, 0.68),
    point("snake_lower_left", "Lower left", 0.12, 0.68),
    point("snake_bottom_left", "Bottom left", 0.12, 0.86),
    point("snake_bottom_mid_left", "Bottom row", 0.32, 0.86),
    point("snake_bottom_center", "Bottom center", 0.5, 0.86),
    point("snake_bottom_mid_right", "Bottom row", 0.68, 0.86),
    point("snake_bottom_right", "Bottom right", 0.88, 0.86),
  ];
}

export function qualityLabel(quality: string | null | undefined) {
  if (!quality) return "Pending";
  return quality.split("_").join(" ");
}

export function emitToast(detail: ObserverAppToastDetail) {
  window.dispatchEvent(new CustomEvent(OBSERVER_APP_TOAST_EVENT, { detail }));
}
