import { invoke } from "@tauri-apps/api/core";

export interface AppStatus {
  name: string;
  version: string;
  platform: string;
}

export function getAppStatus(): Promise<AppStatus> {
  return invoke<AppStatus>("app_get_status");
}
