/** 诊断导出与检查更新的前端 API，对应 `src-tauri/src/commands/diagnostics.rs`。 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ExportOutcome = "saved" | "cancelled" | "failed";

export type UpdateStatus =
  | { state: "idle" }
  | { state: "upToDate"; latestVersion: string }
  | { state: "available"; latestVersion: string }
  | { state: "failed" }
  | { state: "rateLimited" };

export const EVENT_UPDATE_STATUS = "update://status";

export function exportDiagnostics(): Promise<ExportOutcome> {
  return invoke<ExportOutcome>("diagnostics_export");
}

export function revealLogFolder(): Promise<boolean> {
  return invoke<boolean>("diagnostics_reveal_logs");
}

export function checkForUpdates(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("update_check");
}

export function getUpdateStatus(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("update_status");
}

export function openReleasePage(): Promise<boolean> {
  return invoke<boolean>("update_open_release");
}

export function onUpdateStatus(handler: (status: UpdateStatus) => void): Promise<UnlistenFn> {
  return listen<UpdateStatus>(EVENT_UPDATE_STATUS, (event) => handler(event.payload));
}

/** 各服务当前凭据来源的语义名，不含路径、账号或秘密。 */
export interface CredentialSources {
  claudeCode: "file" | "keychain" | "desktop" | "none" | "unreadable" | "expired";
  claudeDesktop: boolean;
  commandCode:
    "commandcode" | "pi" | "opencode" | "env" | "keychain" | "none" | "unreadable" | "expired";
}

export function getCredentialSources(): Promise<CredentialSources> {
  return invoke<CredentialSources>("credential_sources_get");
}
