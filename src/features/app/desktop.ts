import { invoke } from "@tauri-apps/api/core";

export function openMainWindow(): Promise<void> {
  return invoke("window_open_main");
}

export function openSettingsWindow(): Promise<void> {
  return invoke("window_open_settings");
}

export function quitApp(): Promise<void> {
  return invoke("app_quit");
}
