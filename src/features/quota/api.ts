import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { ProviderId, QuotaState, RefreshStatePayload } from "./contracts";

export const EVENT_QUOTA_UPDATED = "quota://updated";
export const EVENT_QUOTA_REFRESH_STATE = "quota://refresh-state";

export function getQuotaSnapshot(): Promise<QuotaState> {
  return invoke<QuotaState>("quota_get_snapshot");
}

/**
 * 请求一次刷新。省略 `provider` 时刷新全部。
 *
 * 请求合并、节流与退避都在 Rust 侧决定：本函数返回不代表真的发起了请求，
 * 界面应当等 `quota://updated` 而不是等这个 Promise。
 */
export function refreshQuota(provider?: ProviderId): Promise<void> {
  return invoke("quota_refresh", { provider: provider ?? null });
}

export function onQuotaUpdated(handler: (state: QuotaState) => void): Promise<UnlistenFn> {
  return listen<QuotaState>(EVENT_QUOTA_UPDATED, (event) => handler(event.payload));
}

export function onRefreshState(
  handler: (payload: RefreshStatePayload) => void,
): Promise<UnlistenFn> {
  return listen<RefreshStatePayload>(EVENT_QUOTA_REFRESH_STATE, (event) => handler(event.payload));
}
