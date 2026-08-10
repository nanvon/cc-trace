import { formatUsdNanos } from "../../lib/format";
import type {
  UsageCostTotals,
  UsagePeriodCost,
  UsageProviderCosts,
  UsageSource,
  UsageSummary,
  UsageTokenTotals,
} from "./contracts";

function zeroCost(): UsagePeriodCost {
  return {
    entryCount: 0,
    apiEquivalentCostNanos: 0,
    pricedEntries: 0,
    unpricedEntries: 0,
    assumedGeoEntries: 0,
  };
}

function periodCost(
  summary: UsageSummary | null,
  source: UsageSource,
  authoritativeEmpty: boolean,
): UsagePeriodCost | null {
  const row = summary?.rows.find((candidate) => candidate.key === source);
  if (row) {
    return {
      entryCount: row.entryCount,
      apiEquivalentCostNanos: row.cost.apiEquivalentCostNanos,
      pricedEntries: row.cost.pricedEntries,
      unpricedEntries: row.cost.unpricedEntries,
      assumedGeoEntries: row.cost.assumedGeoEntries,
    };
  }
  return summary && authoritativeEmpty ? zeroCost() : null;
}

export interface UsageCostDisplay {
  amountNanos: number | null;
}

/**
 * Popover 展示当前可计算出的金额，不追加 `+`，也不额外暴露未定价状态。范围内没有
 * 任何可定价记录时仍显示占位符，避免把全未定价误报成 `$0`。
 */
export function presentUsageCost(cost: UsagePeriodCost | null): UsageCostDisplay {
  if (!cost || (cost.entryCount > 0 && cost.pricedEntries === 0)) {
    return { amountNanos: null };
  }

  return { amountNanos: cost.apiEquivalentCostNanos };
}

export function buildProviderCosts(
  source: UsageSource,
  today: UsageSummary | null,
  week: UsageSummary | null,
  authoritativeEmpty: boolean,
): UsageProviderCosts {
  return {
    today: periodCost(today, source, authoritativeEmpty),
    week: periodCost(week, source, authoritativeEmpty),
  };
}

export interface UsageTokenDisplay {
  value: string;
  unit: string;
  full: string;
}

function compactNumber(
  locale: string,
  value: number,
  unit: string,
  scale: number,
  maximumFractionDigits = 0,
): UsageTokenDisplay {
  const scaled = Math.max(0, value) / scale;
  return {
    value: new Intl.NumberFormat(locale, {
      maximumFractionDigits,
      minimumFractionDigits: 0,
    }).format(scaled),
    unit,
    full: new Intl.NumberFormat(locale).format(Math.max(0, value)),
  };
}

/** 主窗口用量页的 Token 读数：中文使用万／亿，英文使用 K／M／B，紧凑值保留两位小数（去尾零）。 */
export function presentUsageTokens(locale: string, value: number): UsageTokenDisplay {
  if (locale.toLowerCase().startsWith("zh")) {
    if (value >= 100_000_000) return compactNumber(locale, value, "亿", 100_000_000, 2);
    if (value >= 10_000) return compactNumber(locale, value, "万", 10_000, 2);
    return compactNumber(locale, value, "", 1);
  }

  if (value >= 1_000_000_000) return compactNumber(locale, value, "B", 1_000_000_000, 2);
  if (value >= 1_000_000) return compactNumber(locale, value, "M", 1_000_000, 2);
  if (value >= 1_000) return compactNumber(locale, value, "K", 1_000, 2);
  return compactNumber(locale, value, "", 1);
}

/** 缓存写入是输入的一部分，保持与现有草图的 84.0% / 86.2% 口径一致。 */
export function usageCacheHitRate(tokens: UsageTokenTotals): number | null {
  if (tokens.inputTokens <= 0) return null;
  return Math.min(100, (tokens.cacheReadInputTokens / tokens.inputTokens) * 100);
}

/** 该源 Token 占可见源合计的比例（0~100）；合计为 0 时返回 null，展示为「—」。 */
export function usageTokenShare(tokens: UsageTokenTotals, totalTokens: number): number | null {
  if (totalTokens <= 0) return null;
  return Math.min(100, (tokens.totalTokens / totalTokens) * 100);
}

export function formatUsagePercent(locale: string, value: number | null): string {
  if (value === null) return "—";
  return new Intl.NumberFormat(locale, {
    maximumFractionDigits: 1,
    minimumFractionDigits: 1,
    style: "percent",
  }).format(value / 100);
}

export function usageCacheWriteTokens(tokens: UsageTokenTotals): number {
  return tokens.cacheWrite5mInputTokens + tokens.cacheWrite1hInputTokens;
}

export function formatUsageCost(
  locale: string,
  cost: UsageCostTotals | null,
  entryCount: number,
  lessThanCent: string,
): string | null {
  if (!cost || entryCount === 0) return null;
  if (entryCount > 0 && cost.pricedEntries === 0) return null;
  if (cost.apiEquivalentCostNanos > 0 && cost.apiEquivalentCostNanos < 10_000_000) {
    return lessThanCent;
  }
  return formatUsdNanos(locale, cost.apiEquivalentCostNanos);
}
