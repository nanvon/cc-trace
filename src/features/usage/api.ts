import { invoke } from "@tauri-apps/api/core";

import type {
  PricingCatalogRefreshStatus,
  QuotaHistory,
  QuotaHistoryQuery,
  UsageScanStatus,
  UsageSummary,
  UsageSummaryQuery,
} from "./contracts";

export function getUsageScanStatus(): Promise<UsageScanStatus> {
  return invoke<UsageScanStatus>("usage_scan_status");
}

export function startUsageScan(): Promise<UsageScanStatus> {
  return invoke<UsageScanStatus>("usage_scan_start");
}

export function cancelUsageScan(): Promise<UsageScanStatus> {
  return invoke<UsageScanStatus>("usage_scan_cancel");
}

export function getUsageSummary(query: UsageSummaryQuery): Promise<UsageSummary> {
  return invoke<UsageSummary>("usage_get_summary", { query });
}

export function getQuotaHistory(query: QuotaHistoryQuery): Promise<QuotaHistory> {
  return invoke<QuotaHistory>("usage_get_quota_history", { query });
}

export function refreshPricingCatalog(): Promise<PricingCatalogRefreshStatus> {
  return invoke<PricingCatalogRefreshStatus>("usage_refresh_pricing_catalog");
}
