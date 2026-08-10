import { invoke } from "@tauri-apps/api/core";

import type {
  PricingCatalogRefreshStatus,
  QuotaHistory,
  QuotaHistoryQuery,
  UsageConversation,
  UsageConversationBreakdown,
  UsageConversationPage,
  UsageConversationProjectOption,
  UsageConversationQuery,
  UsageScanStatus,
  UsageSource,
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

export function listConversations(query: UsageConversationQuery): Promise<UsageConversationPage> {
  return invoke<UsageConversationPage>("usage_list_conversations", { query });
}

export function listConversationProjects(
  query: UsageConversationQuery,
): Promise<UsageConversationProjectOption[]> {
  return invoke<UsageConversationProjectOption[]>("usage_list_conversation_projects", { query });
}

export function getConversation(conversationKey: string): Promise<UsageConversation | null> {
  return invoke<UsageConversation | null>("usage_get_conversation", { conversationKey });
}

export function getConversationBreakdown(
  conversationKey: string,
): Promise<UsageConversationBreakdown | null> {
  return invoke<UsageConversationBreakdown | null>("usage_get_conversation_breakdown", {
    conversationKey,
  });
}

export function refreshPricingCatalog(): Promise<PricingCatalogRefreshStatus> {
  return invoke<PricingCatalogRefreshStatus>("usage_refresh_pricing_catalog");
}

/**
 * 重建指定数据源：清空其条目与扫描水位后全量重扫。`sources` 缺省时重建全部。
 * 返回启动后的扫描状态，UI 通过轮询 `usage_scan_status` 感知完成。
 */
export function rebuildUsageData(sources?: UsageSource[]): Promise<UsageScanStatus> {
  return invoke<UsageScanStatus>("usage_rebuild_data", { sources: sources ?? null });
}
