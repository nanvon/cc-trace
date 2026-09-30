/**
 * 额度展示契约。与 `src-tauri/src/contracts/quota.rs` 一一对应，改一侧必须同时改另一侧。
 *
 * 三个状态维度独立存在，前端**不得**把它们压回一个互斥枚举，也不得通过 `null`、
 * 空数组或 `0` 推断状态，见 `docs/状态与错误模型.md` 第 1 节。
 */

export type ProviderId = "codex" | "claude" | "antigravity" | "cursor" | "commandCode";

/** 额度主体类型：主账号或用户导入的副账号（目前只有 Codex 支持导入多账号）。 */
export type QuotaSubjectKind = "primary" | "imported";

/** 活动维度：现在是否正在工作。 */
export type RefreshState = "idle" | "loading" | "refreshing";

/** 快照新鲜度维度：当前展示的数据是否可信、是否为旧数据。 */
export type SnapshotFreshness = "empty" | "live" | "stale";

/** 可用性维度：为什么能或不能取得新数据。 */
export type ProviderAvailability =
  "ready" | "no_credentials" | "unsupported" | "offline" | "rate_limited" | "error";

export type QuotaWindowKind =
  "fiveHour" | "weekly" | "modelWeekly" | "monthly" | "total" | "auto" | "api" | "unknown";

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
  /** 是否为返回顺序中的第一项；仅作契约标记，展示主次始终以 `windows` 顺序为准。 */
  isPrimary: boolean;
  /** 无上限额度（Cursor Unlimited）：显示 `∞`，不渲染伪造百分比。 */
  unlimited: boolean;
}

export interface QuotaSnapshot {
  windows: QuotaWindow[];
  capturedAt: string;
}

export interface ProviderIdentity {
  /** 完整账号（邮箱或 account id），见 ADR-0025；隐私模式开启时由前端隐藏显示。 */
  account: string | null;
  plan: string | null;
}

/** 额度主体描述，用于导入账号的增删与排序。 */
export interface QuotaSubject {
  subjectId: string;
  provider: ProviderId;
  kind: QuotaSubjectKind;
  label: string | null;
  orderIndex: number;
}

export interface ProviderSnapshot {
  /** 额度主体标识：主账号是 Provider 短名，导入账号是 `codex:imported:<n>`。 */
  subjectId: string;
  provider: ProviderId;
  kind: QuotaSubjectKind;
  /** 导入账号的展示名；主账号为 `null`，账号在 `identity` 里。 */
  label: string | null;
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
  subjectId: string;
  provider: ProviderId;
  refresh: RefreshState;
}

/** Provider 的空间顺序永远稳定，不随风险重排。 */
export const PROVIDER_ORDER: readonly ProviderId[] = [
  "codex",
  "claude",
  "antigravity",
  "cursor",
  "commandCode",
] as const;

/** Provider 返回的第一项就是主要额度，不按类型或 `isPrimary` 重新排序。 */
export function primaryWindow(snapshot: QuotaSnapshot | null): QuotaWindow | null {
  if (!snapshot || snapshot.windows.length === 0) {
    return null;
  }
  return snapshot.windows[0];
}

/** 第一项之后的额度保持 Provider 返回顺序，作为次级数据。 */
export function secondaryWindows(snapshot: QuotaSnapshot | null): QuotaWindow[] {
  if (!snapshot || snapshot.windows.length <= 1) {
    return [];
  }
  return snapshot.windows.slice(1);
}
