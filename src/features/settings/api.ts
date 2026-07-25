import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { AppStatus, Settings, SettingsUpdate } from "./contracts";

export const EVENT_SETTINGS_UPDATED = "settings://updated";

export function getAppStatus(): Promise<AppStatus> {
  return invoke<AppStatus>("app_get_status");
}

export function readSettings(): Promise<Settings> {
  return invoke<Settings>("settings_read");
}

/** 写入失败时 Rust 保留原值并抛出 `CommandError`，界面必须回滚显示并提示。 */
export function updateSettings(update: SettingsUpdate): Promise<Settings> {
  return invoke<Settings>("settings_update", { update });
}

export function completeOnboarding(): Promise<Settings> {
  return invoke<Settings>("onboarding_complete");
}

export function onSettingsUpdated(handler: (settings: Settings) => void): Promise<UnlistenFn> {
  return listen<Settings>(EVENT_SETTINGS_UPDATED, (event) => handler(event.payload));
}
