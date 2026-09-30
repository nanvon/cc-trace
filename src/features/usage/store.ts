import { defineStore } from "pinia";
import { computed, ref } from "vue";

import type { ProviderId } from "../quota/contracts";
import { useSettingsStore } from "../settings/store";
import { getUsageScanStatus, getUsageSummary, listConversations, startUsageScan } from "./api";
import type {
  UsageConversation,
  UsageDashboardData,
  UsageDashboardRange,
  UsageGranularity,
  UsageGroupBy,
  UsageProviderCosts,
  UsageScanStatus,
  UsageSource,
  UsageSummary,
  UsageSummaryQuery,
} from "./contracts";
import { USAGE_SOURCES } from "./contracts";
import { EMPTY_PROVIDER_COSTS, buildProviderCosts } from "./presentation";
import { conversationSortFor } from "./overview";
import {
  usageChartRange,
  usageCostRanges,
  usageDashboardRanges,
  usageGranularityPresets,
  usagePreviousRange,
} from "./ranges";

function summaryQuery(
  range: Pick<UsageDashboardRange, "from" | "to">,
  groupBy: UsageGroupBy,
  source: UsageSource | null = null,
): UsageSummaryQuery {
  return {
    filter: {
      from: range.from,
      to: range.to,
      sources: source === null ? null : [source],
      model: null,
      speed: null,
      project: null,
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
      dsh: null,
      cursor: null,
    },
    model: {
      codex: null,
      claude: null,
      pi: null,
      opencode: null,
      dsh: null,
      cursor: null,
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
  /**
   * 统计粒度（日／周／月）：与 `dashboardRange` 一起构成概览、对话、项目三页共享的
   * 「粒度＋范围」状态，切换视图不丢失。全局内存态，不持久化。
   */
  const granularity = ref<UsageGranularity>("day");
  /** 概览「用量构成」的项目维度：按项目分组的汇总（键空串＝未归属）。 */
  const overviewProjects = ref<UsageSummary | null>(null);
  /** 概览「高消耗对话」：范围内按排行口径取前 5。 */
  const overviewTopConversations = ref<UsageConversation[]>([]);
  const overviewLoading = ref(false);
  let overviewRequest = 0;
  const dashboardLoaded = ref(false);
  const dashboardLoading = ref(false);
  const dashboardUnavailable = ref(false);
  /** 前一个等长区间（delta 对比用）；`all`/`custom` 时为 null。 */
  const dashboardPrevious = ref<UsageSummary | null>(null);
  let dashboardRequest = 0;

  /** 统计服务过滤：设置页关闭的服务从用量页、图表与对话列表统一剔除。 */
  const visibleSources = computed<UsageSource[]>(() => {
    const services = settings.settings?.services;
    if (!services) return [...USAGE_SOURCES];
    const visible: UsageSource[] = [];
    if (services.codex.stats) visible.push("codex");
    if (services.claude.stats) visible.push("claude");
    if (services.localAgentStats) visible.push("pi", "opencode", "dsh");
    if (services.cursor.stats) visible.push("cursor");
    return visible;
  });

  /**
   * 侧边栏数据源选中态：全局内存态，跨用量／对话／时间线共享，重启回到「全部」。
   * 与设置页「统计服务」开关不同维度——开关决定哪些服务参与统计，这里决定当前查看哪一个。
   * 不持久化（ADR-0024）。
   */
  const sourceFilter = ref<"all" | UsageSource>("all");

  /** 侧边栏可选项：从「可见服务」派生，选中的源必须是可见源。 */
  const sourceFilterOptions = computed<Array<"all" | UsageSource>>(() => [
    "all",
    ...visibleSources.value,
  ]);

  /** 实际参与查询的源集合：选中单个源时收窄到该源。 */
  const dashboardSources = computed<UsageSource[]>(() => {
    if (sourceFilter.value === "all") return visibleSources.value;
    return visibleSources.value.includes(sourceFilter.value) ? [sourceFilter.value] : [];
  });

  function selectSource(source: "all" | UsageSource): void {
    if (source !== "all" && !visibleSources.value.includes(source)) {
      sourceFilter.value = "all";
      return;
    }
    sourceFilter.value = source;
  }

  /** 可见服务的源级汇总：按当前数据源过滤集合（侧边栏选中单源时只计该源）归并。 */
  const visibleSourceSummary = computed<UsageSummary | null>(() => {
    const raw = dashboard.value.source;
    if (!raw) return null;
    const visible = new Set(dashboardSources.value);
    const rows = raw.rows.filter((row) => visible.has(row.key as UsageSource));
    if (rows.length === 0) return null;
    const requestCount = rows.reduce((sum, row) => sum + row.requestCount, 0);

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

    return { rows, entryCount, requestCount, tokens, fast, cost };
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
    const chartRange = usageChartRange(range, granularity.value);
    const previousRange = usagePreviousRange(range);

    await readStatus();

    const sources = dashboardSources.value;
    const queries = [
      getUsageSummary(summaryQuery(range, "source")),
      ...(previousRange ? [getUsageSummary(summaryQuery(previousRange, "source"))] : []),
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
    // `previousRange` 只在有业务意义时存在（`all`/`custom` 为 null），
    // 结果数组因此少一项，索引必须按实际偏移，不能写死从 2 开始。
    const previousOffset = previousRange ? 1 : 0;
    sources.forEach((source, index) => {
      day[source] = value(1 + previousOffset + index);
      model[source] = value(1 + previousOffset + sources.length + index);
    });

    dashboard.value = {
      source: value(0),
      day,
      model,
    };
    dashboardPrevious.value = previousRange ? value(1) : null;
    dashboardUnavailable.value = results.some((result) => result.status === "rejected");
    dashboardLoaded.value = true;
    dashboardLoading.value = false;
  }

  /**
   * 切换粒度：当前范围不在新粒度的可选组里（`all`／`custom` 三个粒度共用）时，
   * 回到新粒度的第一档（当前周期），然后重载 Dashboard。
   */
  async function setGranularity(next: UsageGranularity): Promise<void> {
    if (granularity.value === next) return;
    granularity.value = next;
    const current = dashboardRange.value;
    const presets = usageGranularityPresets(next);
    const keep = current.preset === "custom" || presets.some((preset) => preset === current.preset);
    const range = keep ? current : usageDashboardRanges()[presets[0] ?? "today"];
    await loadDashboard(range);
  }

  /**
   * 概览独有的两组数据：项目维度构成与高消耗对话。与 `loadDashboard` 分开，
   * 对话页、项目页切换范围时不必多做这两次查询。
   */
  async function loadOverview(range: UsageDashboardRange, basis: "tokens" | "cost"): Promise<void> {
    const request = ++overviewRequest;
    overviewLoading.value = true;
    const sources = dashboardSources.value;
    if (sources.length === 0) {
      overviewProjects.value = null;
      overviewTopConversations.value = [];
      overviewLoading.value = false;
      return;
    }
    const filter = {
      from: range.from,
      to: range.to,
      sources,
      model: null,
      speed: null,
      project: null,
    } as const;
    const [projects, conversations] = await Promise.allSettled([
      getUsageSummary({ filter: { ...filter, sources: [...sources] }, groupBy: "project" }),
      listConversations({
        filter: { ...filter, sources: [...sources] },
        search: null,
        project: null,
        sort: conversationSortFor(basis),
        limit: 5,
        offset: 0,
      }),
    ]);
    if (request !== overviewRequest) return;
    overviewProjects.value = projects.status === "fulfilled" ? projects.value : null;
    overviewTopConversations.value =
      conversations.status === "fulfilled" ? conversations.value.items : [];
    overviewLoading.value = false;
  }

  /** 手动触发一次增量扫描；返回是否已启动（扫描中或启动成功）。 */
  async function startScan(): Promise<boolean> {
    if (scanning.value) return false;
    try {
      status.value = await startUsageScan();
      return true;
    } catch {
      statusUnavailable.value = true;
      return false;
    }
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
    // 这三个服务没有本地用量数据源，费用读数恒为空（服务端计量在后续批次接入）。
    antigravity: EMPTY_PROVIDER_COSTS,
    cursor: EMPTY_PROVIDER_COSTS,
    commandCode: EMPTY_PROVIDER_COSTS,
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
    sourceFilter,
    sourceFilterOptions,
    dashboardSources,
    selectSource,
    dashboardRange,
    granularity,
    setGranularity,
    overviewProjects,
    overviewTopConversations,
    overviewLoading,
    loadOverview,
    dashboardLoaded,
    dashboardLoading,
    dashboardUnavailable,
    dashboardPrevious,
    load,
    loadDashboard,
    startScan,
    poll,
  };
});
