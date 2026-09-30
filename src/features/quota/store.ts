import { defineStore } from "pinia";
import { computed, ref } from "vue";

import { presentOverall } from "../../lib/status";
import { useSettingsStore } from "../settings/store";
import { getQuotaSnapshot, refreshQuota } from "./api";
import {
  PROVIDER_ORDER,
  type ProviderId,
  type ProviderSnapshot,
  type QuotaState,
  type RefreshStatePayload,
} from "./contracts";

export const useQuotaStore = defineStore("quota", () => {
  const settings = useSettingsStore();
  const providers = ref<ProviderSnapshot[]>([]);
  const loaded = ref(false);

  /** 空间顺序永远稳定，风险只改变视觉权重。
   * Provider 顺序固定，同一 Provider 内主账号在前、导入账号在后（Rust 已排好，这里只做防御性重排）。 */
  const ordered = computed(() => {
    const rank = new Map(PROVIDER_ORDER.map((id, index) => [id, index]));
    return [...providers.value].sort((left, right) => {
      const providerDelta =
        (rank.get(left.provider) ?? PROVIDER_ORDER.length) -
        (rank.get(right.provider) ?? PROVIDER_ORDER.length);
      if (providerDelta !== 0) return providerDelta;
      if (left.kind !== right.kind) return left.kind === "primary" ? -1 : 1;
      return left.subjectId.localeCompare(right.subjectId);
    });
  });

  /**
   * 界面可见的额度主体。
   *
   * 设置里关闭「额度」的服务不显示——它的轮询也停了，留着只会显示一份过时快照。
   * 设置还没加载时按「全部可见」处理，避免启动瞬间整片空白闪一下。
   */
  const visible = computed(() => {
    const services = settings.settings?.services;
    if (!services) return ordered.value;
    return ordered.value.filter((provider) => services[provider.provider].quota);
  });

  const overall = computed(() => presentOverall(visible.value));

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
      provider.subjectId === payload.subjectId
        ? { ...provider, refresh: payload.refresh }
        : provider,
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
    visible,
    overall,
    busy,
    load,
    adopt,
    adoptRefreshState,
    refresh,
  };
});
