import { defineStore } from "pinia";
import { computed, ref } from "vue";

import { presentOverall } from "../../lib/status";
import { getQuotaSnapshot, refreshQuota } from "./api";
import {
  PROVIDER_ORDER,
  type ProviderId,
  type ProviderSnapshot,
  type QuotaState,
  type RefreshStatePayload,
} from "./contracts";

export const useQuotaStore = defineStore("quota", () => {
  const providers = ref<ProviderSnapshot[]>([]);
  const loaded = ref(false);

  /** 空间顺序永远稳定，风险只改变视觉权重。 */
  const ordered = computed(() =>
    PROVIDER_ORDER.map((id) => providers.value.find((provider) => provider.provider === id)).filter(
      (provider): provider is ProviderSnapshot => provider !== undefined,
    ),
  );

  const overall = computed(() => presentOverall(ordered.value));

  const busy = computed(() => providers.value.some((provider) => provider.refresh !== "idle"));

  async function load(): Promise<void> {
    providers.value = (await getQuotaSnapshot()).providers;
    loaded.value = true;
  }

  /** 采纳 `quota://updated` 的完整状态。前端不做增量合并。 */
  function adopt(state: QuotaState): void {
    providers.value = state.providers;
    loaded.value = true;
  }

  /**
   * 采纳 `quota://refresh-state`，只更新活动维度。
   * 快照与失败原因保持不变——刷新开始不得清空已有数据。
   */
  function adoptRefreshState(payload: RefreshStatePayload): void {
    providers.value = providers.value.map((provider) =>
      provider.provider === payload.provider ? { ...provider, refresh: payload.refresh } : provider,
    );
  }

  /** 请求刷新。真正是否发起由 Rust 的合并、节流与退避决定。 */
  function refresh(provider?: ProviderId): Promise<void> {
    return refreshQuota(provider);
  }

  return {
    providers,
    loaded,
    ordered,
    overall,
    busy,
    load,
    adopt,
    adoptRefreshState,
    refresh,
  };
});
