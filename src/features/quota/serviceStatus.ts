/**
 * 官方服务状态（Statuspage 状态链）契约、命令与状态。
 *
 * 与 `src-tauri/src/contracts/service_status.rs` 一一对应，改一侧必须同时改另一侧。
 * 它与额度三维状态是**两条独立状态链**（ADR-0026）：Statuspage 报告官方服务的公开故障，
 * 不进入 `ProviderSnapshot`、Overall Signal 与退避语义，也不得用 `null` 推断额度状态。
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { defineStore } from "pinia";
import { ref } from "vue";

export type ServiceStatusIndicator =
  "none" | "minor" | "major" | "critical" | "maintenance" | "unknown";

export interface ServiceStatus {
  indicator: ServiceStatusIndicator;
  /** Statuspage 的 `status.description`；缺失时回退到 indicator 文案。 */
  description: string | null;
  /** Statuspage 的 `page.updated_at`，ISO 8601 UTC；缺失时不显示更新时间。 */
  updatedAt: string | null;
  /** 本地抓取时刻，ISO 8601 UTC。 */
  fetchedAt: string;
}

/** `service_status_get` 的返回值与 `service-status://updated` 载荷。 */
export interface ServiceStatusState {
  codex: ServiceStatus | null;
  claude: ServiceStatus | null;
}

export const EVENT_SERVICE_STATUS_UPDATED = "service-status://updated";

export function getServiceStatus(): Promise<ServiceStatusState> {
  return invoke<ServiceStatusState>("service_status_get");
}

export function onServiceStatusUpdated(
  handler: (state: ServiceStatusState) => void,
): Promise<() => void> {
  return listen<ServiceStatusState>(EVENT_SERVICE_STATUS_UPDATED, (event) =>
    handler(event.payload),
  );
}

export const useServiceStatusStore = defineStore("serviceStatus", () => {
  const state = ref<ServiceStatusState>({ codex: null, claude: null });
  const loaded = ref(false);

  function load(): Promise<void> {
    return getServiceStatus().then((value) => {
      state.value = value;
      loaded.value = true;
    });
  }

  /** 采纳 `service-status://updated` 的完整状态。失败保留旧值由 Rust 保证。 */
  function adopt(value: ServiceStatusState): void {
    state.value = value;
    loaded.value = true;
  }

  return { state, loaded, load, adopt };
});
