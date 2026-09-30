/**
 * 本地用量展示契约。与 `src-tauri/src/contracts/usage.rs` 中本切片使用的字段一一对应。
 *
 * Popover 只消费按 Provider 聚合的 Token 费用与扫描状态；Conversations、分页和详情
 * 属于后续主窗口切片，不在这里提前镜像。
 */

import type { ProviderId, QuotaWindowKind } from "../quota/contracts";

/**
 * 本地用量数据源。Pi、OpenCode、DSH 无订阅额度，只进本地用量统计；
 * Cursor 没有本机会话，用量来自账号级远端计量。
 */
export type UsageSource = "codex" | "claude" | "pi" | "opencode" | "dsh" | "cursor";

/** 用量页的空间顺序与 Provider 卡／图例／模型分组一致。 */
export const USAGE_SOURCES: readonly UsageSource[] = [
  "codex",
  "claude",
  "pi",
  "opencode",
  "dsh",
  "cursor",
] as const;

/** 需要本地扫描的数据源；Cursor 不在其中。 */
export const LOCAL_SCAN_SOURCES: readonly UsageSource[] = [
  "codex",
  "claude",
  "pi",
  "opencode",
  "dsh",
] as const;

export type UsageScanState = "idle" | "running" | "cancelling";
export type UsageGroupBy =
  "day" | "week" | "month" | "source" | "provider" | "model" | "project" | "speed";

/** 统计粒度。日／周／月决定时间桶，其余是构成维度。 */
export type UsageGranularity = "day" | "week" | "month";

export interface UsageFilter {
  from: string | null;
  to: string | null;
  /** 可见服务集合；null 表示不过滤，空数组表示一个服务都不可见。 */
  sources: UsageSource[] | null;
  model: string | null;
  speed: "standard" | "fast" | "unknown" | null;
  /** 项目身份键（规范化项目路径）。 */
  project: string | null;
}

export interface UsageSummaryQuery {
  filter: UsageFilter;
  groupBy: UsageGroupBy;
}

export interface UsageTokenTotals {
  uncachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  cacheReadInputTokens: number;
  cacheWrite5mInputTokens: number;
  cacheWrite1hInputTokens: number;
  inputTokens: number;
  totalTokens: number;
}

export interface UsageCostTotals {
  /** 整数 USD nanos；1 USD = 1_000_000_000 nanos。 */
  apiEquivalentCostNanos: number;
  pricedEntries: number;
  unpricedEntries: number;
  assumedGeoEntries: number;
  pricingFingerprint: string | null;
}

export interface UsageFastTotals {
  rawTokens: number;
  /** 十进制定点字符串，避免大 Token 数跨 command 边界丢失精度。 */
  billingEquivalentTokens: string;
  /** 混合模型时显示最小值到最大值；未知倍率为 null。 */
  minimumMultiplier: string | null;
  maximumMultiplier: string | null;
  hasUnpricedEquivalent: boolean;
}

export interface UsageSummaryRow {
  /** Provider、YYYY-MM-DD、YYYY-MM、YYYY-Www 或模型名，取决于 `groupBy`。 */
  key: string;
  /** 事实行数（日粒度远端计量算一行）。 */
  entryCount: number;
  /** 请求数；日粒度远端计量为 0。 */
  requestCount: number;
  tokens: UsageTokenTotals;
  fast: UsageFastTotals;
  cost: UsageCostTotals;
}

export interface UsageSummary {
  rows: UsageSummaryRow[];
  entryCount: number;
  requestCount: number;
  tokens: UsageTokenTotals;
  fast: UsageFastTotals;
  cost: UsageCostTotals;
}

export type PricingCatalogRefreshStatus = "complete" | "partial" | "failed";

export interface UsageScanStatus {
  state: UsageScanState;
  currentSource: UsageSource | null;
  discoveredFiles: number;
  completedFiles: number;
  bytesRead: number;
  insertedEntries: number;
  duplicateEntries: number;
  invalidLines: number;
  failedFiles: number;
  partialFailure: boolean;
  cancelled: boolean;
  startedAt: string | null;
  finishedAt: string | null;
}

/** 单个时间范围、单个 Provider 在 popover 中实际需要的费用事实。 */
export interface UsagePeriodCost {
  entryCount: number;
  apiEquivalentCostNanos: number;
  pricedEntries: number;
  unpricedEntries: number;
  assumedGeoEntries: number;
}

export interface UsageProviderCosts {
  today: UsagePeriodCost | null;
  week: UsagePeriodCost | null;
}

export interface UsageDashboardRange {
  preset:
    | "today"
    | "yesterday"
    | "thisWeek"
    | "thisMonth"
    | "thisYear"
    | "last7Days"
    | "last30Days"
    | "all"
    | "custom";
  from: string | null;
  to: string | null;
}

export interface UsageDashboardData {
  source: UsageSummary | null;
  day: Record<UsageSource, UsageSummary | null>;
  model: Record<UsageSource, UsageSummary | null>;
}

/** 额度历史中的单个事件点。`remainingPercent` 是当时该窗口的整数剩余值。 */
export interface QuotaHistoryEvent {
  provider: ProviderId;
  /** 不可逆身份指纹，只用于把事件归到同一账号序列，不承载账号明文。 */
  identityKey: string;
  windowKind: QuotaWindowKind;
  windowId: string | null;
  remainingPercent: number;
  /** ISO 8601 UTC。 */
  observedAt: string;
  /** 事件时点该窗口的重置时间（ISO 8601 UTC）；缺失或旧数据为 null。 */
  resetsAt: string | null;
}

export interface QuotaHistoryQuery {
  provider: ProviderId | null;
  from: string | null;
  to: string | null;
  limit: number | null;
}

export interface QuotaHistory {
  events: QuotaHistoryEvent[];
}

export type UsageConversationSort = "recent" | "tokens" | "cost";

export interface UsageConversationQuery {
  filter: UsageFilter;
  search: string | null;
  /** 项目身份键（规范化项目路径）。 */
  project: string | null;
  sort: UsageConversationSort | null;
  limit: number | null;
  offset: number | null;
}

export interface UsageConversation {
  conversationKey: string;
  source: UsageSource;
  title: string | null;
  projectHint: string | null;
  /** 项目身份键（规范化项目路径）；未归属或未解析时为 null。 */
  projectKey: string | null;
  /** 对话自身的工作目录路径，用于 worktree 明细。 */
  worktreePath: string | null;
  /** 未归属：Cursor 远端计量与补录历史。不进入对话列表。 */
  unattributed: boolean;
  isSidechain: boolean;
  firstAt: string;
  lastAt: string;
  entryCount: number;
  /** 请求数；日粒度远端计量为 0。 */
  requestCount: number;
  tokens: UsageTokenTotals;
  fast: UsageFastTotals;
  cost: UsageCostTotals;
  /** 原始会话 id（会话 UUID），供详情页展示与复制；非账号明文。 */
  sourceId: string | null;
  /** Claude 会话的 git 分支；Codex 不提供。 */
  branch: string | null;
  /** 会话涉及的去重模型列表，按模型名排序；不受查询过滤影响。 */
  models: string[];
}

/** 对话项目筛选选项：项目身份键、展示名与其可见对话数、最近活动时间。 */
export interface UsageConversationProjectOption {
  key: string | null;
  name: string;
  conversationCount: number;
  lastAt: string;
}

export type UsageProjectSort = "recent" | "tokens" | "cost";

export interface UsageProjectQuery {
  filter: UsageFilter;
  search: string | null;
  sort: UsageProjectSort | null;
  limit: number | null;
  offset: number | null;
}

/** 一个项目的用量汇总。`key` 为空串表示未归属分组。 */
export interface UsageProjectSummary {
  /** 项目身份键（规范化仓库根路径）；未归属分组为空串。 */
  key: string;
  /** 展示名（路径尾段）；未归属分组使用固定文案。 */
  name: string;
  /** 完整项目路径；未归属分组为 null。 */
  path: string | null;
  unattributed: boolean;
  conversationCount: number;
  activeDays: number;
  firstAt: string | null;
  lastAt: string;
  entryCount: number;
  requestCount: number;
  tokens: UsageTokenTotals;
  fast: UsageFastTotals;
  cost: UsageCostTotals;
}

export interface UsageProjectPage {
  items: UsageProjectSummary[];
  total: number;
  limit: number;
  offset: number;
}

export interface UsageConversationPage {
  items: UsageConversation[];
  total: number;
  limit: number;
  offset: number;
}

/** 单个对话详情里的模型／速度拆分行；列布局与 `UsageSummaryRow` 一致。 */
export interface UsageConversationBreakdown {
  models: UsageSummaryRow[];
  speeds: UsageSummaryRow[];
}
