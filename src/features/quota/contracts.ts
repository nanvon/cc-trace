/**
 * 额度展示契约。与 `src-tauri/src/contracts/quota.rs` 一一对应，改一侧必须同时改另一侧。
 *
 * 三个状态维度独立存在，前端**不得**把它们压回一个互斥枚举，也不得通过 `null`、
 * 空数组或 `0` 推断状态，见 `docs/状态与错误模型.md` 第 1 节。
 */

export type ProviderId = "codex" | "claude";

/** 活动维度：现在是否正在工作。 */
export type RefreshState = "idle" | "loading" | "refreshing";

/** 快照新鲜度维度：当前展示的数据是否可信、是否为旧数据。 */
export type SnapshotFreshness = "empty" | "live" | "stale";

/** 可用性维度：为什么能或不能取得新数据。 */
export type ProviderAvailability =
  "ready" | "no_credentials" | "unsupported" | "offline" | "rate_limited" | "error";

export type QuotaWindowKind = "fiveHour" | "weekly" | "modelWeekly" | "unknown";

/** `error` 的两个文案分支：凭据类指向重新登录，协议类指向稍后重试。 */
export type ErrorKind = "credentials" | "protocol";

export interface AppError {
  kind: ErrorKind;
}

export interface QuotaWindow {
  id: string;
  kind: QuotaWindowKind;
  /** 只在 `kind` 无法完整表达时出现，例如 `modelWeekly` 的模型名。 */
  displayName: string | null;
  usedPercent: number;
  /** 已在 Rust 侧 clamp 到 0–100，前端不重算。 */
  remainingPercent: number;
  /** ISO 8601 UTC；缺失时显示「重置时间未知」。 */
  resetsAt: string | null;
  windowSeconds: number | null;
  isActive: boolean;
  /** 主要额度：紧凑面板与主窗口的首要判断依据。 */
  isPrimary: boolean;
}

export interface QuotaSnapshot {
  windows: QuotaWindow[];
  capturedAt: string;
}

export interface ProviderIdentity {
  accountHint: string | null;
  plan: string | null;
}

export interface ProviderSnapshot {
  provider: ProviderId;
  refresh: RefreshState;
  freshness: SnapshotFreshness;
  availability: ProviderAvailability;
  identity: ProviderIdentity | null;
  snapshot: QuotaSnapshot | null;
  lastSuccessAt: string | null;
  lastAttemptAt: string | null;
  /** 退避期内可再次尝试的时刻。手动刷新同样受它约束。 */
  retryAfter: string | null;
  error: AppError | null;
}

export interface QuotaState {
  providers: ProviderSnapshot[];
}

export interface RefreshStatePayload {
  provider: ProviderId;
  refresh: RefreshState;
}

/** Provider 的空间顺序永远稳定，不随风险重排。 */
export const PROVIDER_ORDER: readonly ProviderId[] = ["codex", "claude"] as const;

/** 主要额度窗口。缺失时退到第一个窗口，不返回 `undefined` 让调用方猜。 */
export function primaryWindow(snapshot: QuotaSnapshot | null): QuotaWindow | null {
  if (!snapshot || snapshot.windows.length === 0) {
    return null;
  }
  return snapshot.windows.find((window) => window.isPrimary) ?? snapshot.windows[0];
}

/** 除主要额度外的必要专项额度。 */
export function secondaryWindows(snapshot: QuotaSnapshot | null): QuotaWindow[] {
  const primary = primaryWindow(snapshot);
  if (!snapshot || !primary) {
    return [];
  }
  return snapshot.windows.filter((window) => window.id !== primary.id);
}
