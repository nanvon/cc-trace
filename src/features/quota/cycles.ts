/**
 * 额度周期（额度页上半部分）的视图模型。
 *
 * 与 `src-tauri/src/contracts/usage.rs` 的周期契约一一对应。周期是从 `quota_events`
 * 现算的派生结果，不是另一份落盘状态：同一份事件在两次查询之间给出同一组周期，
 * 因此这里的归组只做展示层的事（排序、主体分区、窗口分档）。
 */
import type { ProviderId, QuotaWindowKind } from "./contracts";
import type {
  QuotaCycleForecast,
  QuotaCyclePage,
  QuotaCycleSegmentView,
  QuotaCycleUsage,
  QuotaCycleView,
} from "../usage/contracts";

export type { QuotaCycleForecast, QuotaCycleSegmentView, QuotaCycleUsage, QuotaCycleView };

/** 一个额度主体（主账号或导入账号）的全部窗口周期。 */
export interface QuotaCycleAccount {
  provider: ProviderId;
  identityKey: string;
  /** 导入账号的别名或邮箱；主账号为 null，界面按服务名显示。 */
  label: string | null;
  /** 按窗口分档，短窗口在前。 */
  windows: QuotaCycleWindow[];
}

/** 一个窗口的周期序列（含历史周期，最后一个通常是当前周期）。 */
export interface QuotaCycleWindow {
  windowKind: QuotaWindowKind;
  windowId: string;
  windowSeconds: number | null;
  /** 周期起点升序。 */
  cycles: QuotaCycleView[];
}

/** 窗口在展示顺序里的位置：短窗口在前，长窗口在后。 */
export function windowRank(kind: QuotaWindowKind): number {
  switch (kind) {
    case "fiveHour":
      return 0;
    case "weekly":
      return 1;
    case "modelWeekly":
      return 2;
    case "monthly":
      return 3;
    case "total":
      return 4;
    case "auto":
      return 5;
    case "api":
      return 6;
    default:
      return 7;
  }
}

/** 窗口徽标短标签：固定不翻译（对齐 cc-bar 的 `limitKindLabel`）。 */
export function windowBadge(kind: QuotaWindowKind): string {
  switch (kind) {
    case "fiveHour":
      return "5H";
    case "weekly":
      return "WK";
    case "modelWeekly":
      return "MODEL";
    case "monthly":
      return "MONTHLY";
    case "total":
      return "TOTAL";
    case "auto":
      return "AUTO";
    case "api":
      return "API";
    default:
      return "CURRENT";
  }
}

/**
 * 按主体与窗口归组，顺序稳定：服务顺序 → 主体（主账号在前）→ 窗口（短窗口在前）。
 *
 * `page.cycles` 已经按服务与窗口排过序，这里只按主体分区：同一个服务的导入账号
 * 各自成区，不混在一张账上。
 */
export function groupCycles(page: QuotaCyclePage): QuotaCycleAccount[] {
  const accounts = new Map<string, QuotaCycleAccount>();
  const order: string[] = [];

  for (const cycle of page.cycles) {
    const key = `${cycle.provider}|${cycle.identityKey}`;
    let account = accounts.get(key);
    if (!account) {
      account = {
        provider: cycle.provider,
        identityKey: cycle.identityKey,
        label: cycle.identityLabel ?? null,
        windows: [],
      };
      accounts.set(key, account);
      order.push(key);
    }
    let window = account.windows.find((candidate) => candidate.windowId === cycle.windowId);
    if (!window) {
      window = {
        windowKind: cycle.windowKind,
        windowId: cycle.windowId,
        windowSeconds: cycle.windowSeconds,
        cycles: [],
      };
      account.windows.push(window);
    }
    window.cycles.push(cycle);
  }

  for (const key of order) {
    const account = accounts.get(key);
    if (!account) continue;
    for (const window of account.windows) {
      window.cycles.sort((left, right) => left.startAt.localeCompare(right.startAt));
    }
    account.windows.sort(
      (left, right) =>
        windowRank(left.windowKind) - windowRank(right.windowKind) ||
        left.windowId.localeCompare(right.windowId),
    );
  }

  return order
    .map((key) => accounts.get(key))
    .filter((account): account is QuotaCycleAccount => account !== undefined);
}

/** 活动周期：`active` 标记的那一个；缺失时取最后一个。 */
export function activeCycle(window: QuotaCycleWindow): QuotaCycleView | null {
  if (window.cycles.length === 0) return null;
  return window.cycles.find((cycle) => cycle.active) ?? window.cycles[window.cycles.length - 1];
}

/** 历史周期（不含活动周期），新的在前。 */
export function pastCycles(window: QuotaCycleWindow): QuotaCycleView[] {
  const active = activeCycle(window);
  return window.cycles
    .filter((cycle) => cycle !== active)
    .sort((left, right) => right.startAt.localeCompare(left.startAt));
}

/** 官方口径的已用比例：取活动片段，缺失时退回整周期读数。 */
export function officialUsedPercent(cycle: QuotaCycleView): number {
  const segment = cycle.segments.find((candidate) => candidate.active);
  return segment ? segment.observedUsedPercent : cycle.latestUsedPercent;
}

/** 周期剩余时长（毫秒）。已过期为负数：调用方按「即将重置」处理。 */
export function remainingMs(cycle: QuotaCycleView, now: number): number {
  return new Date(cycle.endAt).getTime() - now;
}

/** 周期进度（0～1）：已经走过的时间占比。 */
export function cycleProgress(cycle: QuotaCycleView, now: number): number {
  const start = new Date(cycle.startAt).getTime();
  const end = new Date(cycle.endAt).getTime();
  if (!(end > start)) return 1;
  return Math.min(1, Math.max(0, (now - start) / (end - start)));
}

/** 用量读数的 token 总数：与总览的 Tokens 口径一致（reasoning 含在 output 里）。 */
export function usageTokens(usage: QuotaCycleUsage): number {
  const tokens = usage.tokens;
  return (
    tokens.uncachedInputTokens +
    tokens.outputTokens +
    tokens.cacheReadInputTokens +
    tokens.cacheWrite5mInputTokens +
    tokens.cacheWrite1hInputTokens
  );
}
