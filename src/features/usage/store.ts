import { defineStore } from "pinia";
import { computed, ref } from "vue";

import type { ProviderId } from "../quota/contracts";
import { getUsageScanStatus, getUsageSummary } from "./api";
import type {
  UsageProviderCosts,
  UsageScanStatus,
  UsageSummary,
  UsageSummaryQuery,
} from "./contracts";
import { buildProviderCosts } from "./presentation";
import { usageCostRanges } from "./ranges";

function summaryQuery(from: string, to: string): UsageSummaryQuery {
  return {
    filter: {
      from,
      to,
      source: null,
      model: null,
      speed: null,
    },
    groupBy: "source",
  };
}

export const useUsageStore = defineStore("usage", () => {
  const status = ref<UsageScanStatus | null>(null);
  const today = ref<UsageSummary | null>(null);
  const week = ref<UsageSummary | null>(null);
  const loaded = ref(false);
  const statusUnavailable = ref(false);
  const summaryUnavailable = ref(false);
  const completedInSession = ref(false);

  const scanning = computed(
    () => status.value?.state === "running" || status.value?.state === "cancelling",
  );
  /** 首次状态与汇总读取期间也给出反馈，不先闪一帧无状态的占位符。 */
  const loading = computed(() => !loaded.value || scanning.value);
  const partial = computed(() => Boolean(status.value?.partialFailure || status.value?.cancelled));
  const unavailable = computed(() => statusUnavailable.value || summaryUnavailable.value);

  async function readSummaries(now: Date = new Date()): Promise<void> {
    const ranges = usageCostRanges(now);
    const [todayResult, weekResult] = await Promise.allSettled([
      getUsageSummary(summaryQuery(ranges.today.from, ranges.today.to)),
      getUsageSummary(summaryQuery(ranges.week.from, ranges.week.to)),
    ]);

    let failed = false;
    if (todayResult.status === "fulfilled") {
      today.value = todayResult.value;
    } else {
      failed = true;
    }
    if (weekResult.status === "fulfilled") {
      week.value = weekResult.value;
    } else {
      failed = true;
    }
    summaryUnavailable.value = failed;
  }

  async function readStatus(): Promise<void> {
    try {
      status.value = await getUsageScanStatus();
      statusUnavailable.value = false;
      if (status.value.finishedAt) {
        completedInSession.value = true;
      }
    } catch {
      // 不能拿上一轮的 `running` 永久轮询；状态读取失败已经由 unavailable 明示。
      status.value = null;
      statusUnavailable.value = true;
    }
  }

  async function load(now: Date = new Date()): Promise<void> {
    await readStatus();
    if (!scanning.value) {
      await readSummaries(now);
    }
    loaded.value = true;
  }

  /** 扫描没有 event；可见期间只轮询状态，结束后再一次性采纳新的完整汇总。 */
  async function poll(now: Date = new Date()): Promise<boolean> {
    const previousFinishedAt = status.value?.finishedAt ?? null;
    await readStatus();
    const finishedAt = status.value?.finishedAt ?? null;
    // 扫描可能在两次一秒轮询之间开始并结束，不能依赖前端必须先观察到 running。
    if (!scanning.value && finishedAt !== null && finishedAt !== previousFinishedAt) {
      completedInSession.value = true;
      await readSummaries(now);
    }
    return scanning.value;
  }

  const costs = computed<Record<ProviderId, UsageProviderCosts>>(() => ({
    codex: buildProviderCosts("codex", today.value, week.value, completedInSession.value),
    claude: buildProviderCosts("claude", today.value, week.value, completedInSession.value),
  }));

  return {
    status,
    loaded,
    scanning,
    loading,
    partial,
    unavailable,
    costs,
    load,
    poll,
  };
});
