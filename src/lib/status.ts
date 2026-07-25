/**
 * 三个状态维度 → 文案 key、视觉基调与轨道处理的**唯一映射**。
 *
 * 状态语义由 `docs/状态与错误模型.md` 拥有，文案目录由 `docs/文案与国际化.md` 第 4 节
 * 拥有，视觉表达由 `docs/设计方向与状态规范.md` 第 8 节拥有。组件不得自己再判断一遍
 * `availability === "offline"` 之类的分支——那会让状态语义出现第二份实现。
 */

import type { ProviderSnapshot } from "../features/quota/contracts";

/** 视觉基调。颜色永远伴随状态词或说明，不单独承担语义。 */
export type StatusTone = "neutral" | "warning" | "critical";

/** Reset Rail 的轨道处理方式。 */
export type RailTreatment =
  | "filled" /** 当前有效数值 */
  | "faded" /** 旧快照：保留数值，视觉上明确降级 */
  | "loading" /** 首次加载，尚无任何数值 */
  | "empty"; /** 没有可展示的额度，且不是加载中 */

export interface StatusPresentation {
  titleKey: string;
  /** 主窗口展开时的「影响」。紧凑面板不展示。 */
  impactKey: string | null;
  /** 可执行的下一步。没有别的动作时才为 `null`。 */
  nextStepKey: string | null;
  tone: StatusTone;
  rail: RailTreatment;
  /** 风险权重，用于挑出总体状态。数字越大越需要注意，不改变 Provider 的空间顺序。 */
  severity: number;
}

/** 与 `docs/状态与错误模型.md` 第 3 节的状态优先级一一对应。 */
const SEVERITY = {
  credentials: 50,
  failureWithoutSnapshot: 40,
  failureWithSnapshot: 30,
  working: 20,
  live: 10,
} as const;

export function presentProvider(provider: ProviderSnapshot): StatusPresentation {
  const hasSnapshot = provider.freshness !== "empty";

  switch (provider.availability) {
    case "no_credentials":
      return {
        titleKey: "status.noCredentials",
        impactKey: "impact.noCredentials",
        nextStepKey: "nextStep.noCredentials",
        // 没有凭据不是错误，用中性说明与可执行建议，不默认标红。
        tone: "neutral",
        rail: "empty",
        severity: SEVERITY.credentials,
      };

    case "unsupported":
      return {
        titleKey: "status.unsupported",
        impactKey: "impact.unsupported",
        nextStepKey: "nextStep.unsupported",
        tone: "neutral",
        rail: "empty",
        severity: SEVERITY.credentials,
      };

    case "offline":
      return hasSnapshot
        ? {
            titleKey: "status.offlineStale",
            impactKey: "impact.stale",
            nextStepKey: "nextStep.offlineStale",
            tone: "warning",
            rail: "faded",
            severity: SEVERITY.failureWithSnapshot,
          }
        : {
            titleKey: "status.offlineEmpty",
            impactKey: "impact.empty",
            nextStepKey: "nextStep.offlineEmpty",
            tone: "warning",
            rail: "empty",
            severity: SEVERITY.failureWithoutSnapshot,
          };

    case "rate_limited":
      return {
        titleKey: "status.rateLimited",
        impactKey: "impact.rateLimited",
        nextStepKey: "nextStep.rateLimited",
        tone: "warning",
        rail: hasSnapshot ? "faded" : "empty",
        severity: hasSnapshot ? SEVERITY.failureWithSnapshot : SEVERITY.failureWithoutSnapshot,
      };

    case "error": {
      const isCredentialError = provider.error?.kind === "credentials";
      return {
        titleKey: isCredentialError ? "status.errorCredentials" : "status.errorProtocol",
        impactKey: hasSnapshot ? "impact.stale" : "impact.empty",
        nextStepKey: isCredentialError ? "nextStep.errorCredentials" : "nextStep.errorProtocol",
        tone: "critical",
        rail: hasSnapshot ? "faded" : "empty",
        severity: isCredentialError
          ? SEVERITY.credentials
          : hasSnapshot
            ? SEVERITY.failureWithSnapshot
            : SEVERITY.failureWithoutSnapshot,
      };
    }

    case "ready":
    default:
      if (provider.refresh === "refreshing") {
        return {
          titleKey: "status.refreshing",
          impactKey: null,
          nextStepKey: null,
          tone: "neutral",
          rail: provider.freshness === "stale" ? "faded" : "filled",
          severity: SEVERITY.working,
        };
      }
      if (!hasSnapshot) {
        return {
          titleKey: "status.loading",
          impactKey: null,
          nextStepKey: null,
          tone: "neutral",
          rail: "loading",
          severity: SEVERITY.working,
        };
      }
      if (provider.freshness === "stale") {
        return {
          titleKey: "status.stale",
          impactKey: "impact.stale",
          nextStepKey: "nextStep.stale",
          tone: "warning",
          rail: "faded",
          severity: SEVERITY.failureWithSnapshot,
        };
      }
      return {
        titleKey: "status.live",
        impactKey: null,
        nextStepKey: null,
        tone: "neutral",
        rail: "filled",
        severity: SEVERITY.live,
      };
  }
}

/**
 * 总体状态：只回答「现在最需要注意什么」。
 *
 * 相同风险时保留第一个 Provider，因此空间顺序永远稳定，刷新后内容不跳动。
 */
export function presentOverall(providers: ProviderSnapshot[]): {
  provider: ProviderSnapshot;
  presentation: StatusPresentation;
} | null {
  let leader: { provider: ProviderSnapshot; presentation: StatusPresentation } | null = null;

  for (const provider of providers) {
    const presentation = presentProvider(provider);
    if (!leader || presentation.severity > leader.presentation.severity) {
      leader = { provider, presentation };
    }
  }

  return leader;
}

/** 是否有可展示的额度数值。前端不用 `null` 或 `0` 推断这件事。 */
export function hasQuotaValues(provider: ProviderSnapshot): boolean {
  return provider.freshness !== "empty" && provider.snapshot !== null;
}
