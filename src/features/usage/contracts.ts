/**
 * 本地用量展示契约。与 `src-tauri/src/contracts/usage.rs` 中本切片使用的字段一一对应。
 *
 * Popover 只消费按 Provider 聚合的 Token 费用与扫描状态；Conversations、分页和详情
 * 属于后续主窗口切片，不在这里提前镜像。
 */

import type { ProviderId, QuotaWindowKind } from "../quota/contracts";

export type UsageSource = ProviderId;
export type UsageScanState = "idle" | "running" | "cancelling";
export type UsageGroupBy = "day" | "source" | "model" | "speed";

export interface UsageFilter {
  from: string | null;
  to: string | null;
  source: UsageSource | null;
  model: string | null;
  speed: "standard" | "fast" | "unknown" | null;
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
  /** Provider、YYYY-MM-DD 或模型名，取决于 `groupBy`。 */
  key: string;
  entryCount: number;
  tokens: UsageTokenTotals;
  fast: UsageFastTotals;
  cost: UsageCostTotals;
}

export interface UsageSummary {
  rows: UsageSummaryRow[];
  entryCount: number;
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
