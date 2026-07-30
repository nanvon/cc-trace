import type { UsagePeriodCost, UsageProviderCosts, UsageSource, UsageSummary } from "./contracts";

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
