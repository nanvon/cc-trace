import { defineStore } from "pinia";
import { computed, ref } from "vue";

import type { ProviderId } from "../quota/contracts";
import { getUsageScanStatus, getUsageSummary } from "./api";
import type {
  UsageDashboardData,
  UsageDashboardRange,
  UsageGroupBy,
  UsageProviderCosts,
  UsageScanStatus,
  UsageSource,
  UsageSummary,
  UsageSummaryQuery,
} from "./contracts";
import { buildProviderCosts } from "./presentation";
import { usageChartRange, usageCostRanges, usageDashboardRanges } from "./ranges";

function summaryQuery(
  range: Pick<UsageDashboardRange, "from" | "to">,
  groupBy: UsageGroupBy,
  source: UsageSource | null = null,
): UsageSummaryQuery {
  return {
    filter: {
      from: range.from,
      to: range.to,
      source,
      model: null,
      speed: null,
    },
    groupBy,
  };
}

function emptyDashboard(): UsageDashboardData {
  return {
    source: null,
    day: { codex: null, claude: null },
    model: { codex: null, claude: null },
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
  const dashboard = ref<UsageDashboardData>(emptyDashboard());
  const dashboardRange = ref<UsageDashboardRange>(usageDashboardRanges().thisMonth);
  const dashboardLoaded = ref(false);
  const dashboardLoading = ref(false);
  const dashboardUnavailable = ref(false);
  let dashboardRequest = 0;

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
      getUsageSummary(summaryQuery(ranges.today, "source")),
      getUsageSummary(summaryQuery(ranges.week, "source")),
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

  /**
   * 主窗口的单次范围查询。Rust 已支持 day/source/model 三种聚合，按 Provider 拆开 day 与
   * model 查询即可保持现有 command 契约，同时让图表和分组表都能闭合对账。
   */
  async function loadDashboard(range: UsageDashboardRange): Promise<void> {
    const request = ++dashboardRequest;
    dashboardRange.value = range;
    dashboardLoading.value = true;
    dashboardUnavailable.value = false;
    dashboard.value = emptyDashboard();
    const chartRange = usageChartRange(range);

    await readStatus();

    const results = await Promise.allSettled([
      getUsageSummary(summaryQuery(range, "source")),
      getUsageSummary(summaryQuery(chartRange, "day", "codex")),
      getUsageSummary(summaryQuery(chartRange, "day", "claude")),
      getUsageSummary(summaryQuery(range, "model", "codex")),
      getUsageSummary(summaryQuery(range, "model", "claude")),
    ]);

    if (request !== dashboardRequest) {
      return;
    }

    const value = (index: number): UsageSummary | null => {
      const result = results[index];
      if (!result || result.status !== "fulfilled") return null;
      return result.value;
    };

    dashboard.value = {
      source: value(0),
      day: { codex: value(1), claude: value(2) },
      model: { codex: value(3), claude: value(4) },
    };
    dashboardUnavailable.value = results.some((result) => result.status === "rejected");
    dashboardLoaded.value = true;
    dashboardLoading.value = false;
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
    dashboard,
    dashboardRange,
    dashboardLoaded,
    dashboardLoading,
    dashboardUnavailable,
    load,
    loadDashboard,
    poll,
  };
});
