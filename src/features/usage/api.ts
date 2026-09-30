import { invoke } from "@tauri-apps/api/core";

import type {
  PricingCatalogRefreshStatus,
  QuotaCyclePage,
  QuotaCycleQuery,
  QuotaHistory,
  QuotaHistoryQuery,
  UsageConversation,
  UsageConversationBreakdown,
  UsageConversationPage,
  UsageConversationProjectOption,
  UsageConversationQuery,
  UsageProjectBreakdown,
  UsageProjectBreakdownQuery,
  UsageProjectPage,
  UsageProjectQuery,
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

/** 额度周期与用满预估。周期是从额度事件现算的派生结果，不落盘。 */
export function getQuotaCycles(query: QuotaCycleQuery): Promise<QuotaCyclePage> {
  return invoke<QuotaCyclePage>("usage_get_quota_cycles", { query });
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

export function listProjects(query: UsageProjectQuery): Promise<UsageProjectPage> {
  return invoke<UsageProjectPage>("usage_list_projects", { query });
}

export function getProjectBreakdown(
  query: UsageProjectBreakdownQuery,
): Promise<UsageProjectBreakdown> {
  return invoke<UsageProjectBreakdown>("usage_get_project_breakdown", { query });
}

/** 在系统文件管理器中显示项目目录；目录已不存在或不是已知项目路径时返回 false。 */
export function revealProject(path: string): Promise<boolean> {
  return invoke<boolean>("usage_reveal_project", { path });
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
