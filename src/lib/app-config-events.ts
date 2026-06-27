export const OBSERVER_CONFIG_UPDATED_EVENT = "observer-config-updated";
export const OBSERVER_APP_TOAST_EVENT = "observer-app-toast";

export interface ObserverAppToastDetail {
  type: "success" | "error";
  text: string;
}
