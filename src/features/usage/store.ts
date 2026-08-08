import { defineStore } from "pinia";
import { computed, ref } from "vue";

import type { ProviderId } from "../quota/contracts";
import { useSettingsStore } from "../settings/store";
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
import { USAGE_SOURCES } from "./contracts";
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
    day: {
      codex: null,
      claude: null,
      pi: null,
      opencode: null,
    },
    model: {
      codex: null,
      claude: null,
      pi: null,
      opencode: null,
    },
  };
}

export const useUsageStore = defineStore("usage", () => {
  const settings = useSettingsStore();
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

  /** 统计服务过滤：设置页关闭的服务从用量页、图表与对话列表统一剔除。 */
  const visibleSources = computed<UsageSource[]>(() => {
    const visibility = settings.settings?.usageServiceVisibility;
    if (!visibility) return [...USAGE_SOURCES];
    return USAGE_SOURCES.filter((source) => visibility[source]);
  });

  /** 可见服务的源级汇总：从全量 source 查询中按可见集合归并，占比与总量只计可见服务。 */
  const visibleSourceSummary = computed<UsageSummary | null>(() => {
    const raw = dashboard.value.source;
    if (!raw) return null;
    const visible = new Set(visibleSources.value);
    const rows = raw.rows.filter((row) => visible.has(row.key as UsageSource));
    if (rows.length === 0) return null;

    const tokens: UsageSummary["tokens"] = {
      uncachedInputTokens: 0,
      outputTokens: 0,
      reasoningOutputTokens: 0,
      cacheReadInputTokens: 0,
      cacheWrite5mInputTokens: 0,
      cacheWrite1hInputTokens: 0,
      inputTokens: 0,
      totalTokens: 0,
    };
    const fast: UsageSummary["fast"] = {
      rawTokens: 0,
      billingEquivalentTokens: "0",
      minimumMultiplier: null,
      maximumMultiplier: null,
      hasUnpricedEquivalent: false,
    };
    const cost: UsageSummary["cost"] = {
      apiEquivalentCostNanos: 0,
      pricedEntries: 0,
      unpricedEntries: 0,
      assumedGeoEntries: 0,
      pricingFingerprint: null,
    };
    let entryCount = 0;
    for (const row of rows) {
      entryCount += row.entryCount;
      tokens.uncachedInputTokens += row.tokens.uncachedInputTokens;
      tokens.outputTokens += row.tokens.outputTokens;
      tokens.reasoningOutputTokens += row.tokens.reasoningOutputTokens;
      tokens.cacheReadInputTokens += row.tokens.cacheReadInputTokens;
      tokens.cacheWrite5mInputTokens += row.tokens.cacheWrite5mInputTokens;
      tokens.cacheWrite1hInputTokens += row.tokens.cacheWrite1hInputTokens;
      tokens.inputTokens += row.tokens.inputTokens;
      tokens.totalTokens += row.tokens.totalTokens;
      fast.rawTokens += row.fast.rawTokens;
      fast.billingEquivalentTokens = String(
        (Number(fast.billingEquivalentTokens) || 0) +
          (Number(row.fast.billingEquivalentTokens) || 0),
      );
      fast.minimumMultiplier ??= row.fast.minimumMultiplier;
      fast.maximumMultiplier ??= row.fast.maximumMultiplier;
      fast.hasUnpricedEquivalent ||= row.fast.hasUnpricedEquivalent;
      cost.apiEquivalentCostNanos += row.cost.apiEquivalentCostNanos;
      cost.pricedEntries += row.cost.pricedEntries;
      cost.unpricedEntries += row.cost.unpricedEntries;
      cost.assumedGeoEntries += row.cost.assumedGeoEntries;
      cost.pricingFingerprint ??= row.cost.pricingFingerprint;
    }

    return { rows, entryCount, tokens, fast, cost };
  });

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

    const sources = visibleSources.value;
    const queries = [
      getUsageSummary(summaryQuery(range, "source")),
      ...sources.map((source) => getUsageSummary(summaryQuery(chartRange, "day", source))),
      ...sources.map((source) => getUsageSummary(summaryQuery(range, "model", source))),
    ];
    const results = await Promise.allSettled(queries);

    if (request !== dashboardRequest) {
      return;
    }

    const value = (index: number): UsageSummary | null => {
      const result = results[index];
      if (!result || result.status !== "fulfilled") return null;
      return result.value;
    };

    const day: UsageDashboardData["day"] = emptyDashboard().day;
    const model: UsageDashboardData["model"] = emptyDashboard().model;
    sources.forEach((source, index) => {
      day[source] = value(1 + index);
      model[source] = value(1 + sources.length + index);
    });

    dashboard.value = {
      source: value(0),
      day,
      model,
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
    visibleSources,
    visibleSourceSummary,
    dashboardRange,
    dashboardLoaded,
    dashboardLoading,
    dashboardUnavailable,
    load,
    loadDashboard,
    poll,
  };
});
