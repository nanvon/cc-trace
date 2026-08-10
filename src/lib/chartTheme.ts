/**
 * ECharts 不会读取 CSS custom properties，因此主窗口图表只从这里取得主题色。
 * Provider 分类色与状态色保持分离，主题变化时重新计算 option 即可。
 */

import { quotaTone } from "./quotaTone";
export interface UsageChartColors {
  codex: string;
  claude: string;
  pi: string;
  opencode: string;
  fontFamily: string;
  text: string;
  muted: string;
  border: string;
  surface: string;
}

function cssVariable(name: string, fallback: string): string {
  if (typeof document === "undefined") return fallback;
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

export function usageChartColors(): UsageChartColors {
  return {
    codex: cssVariable("--cat-codex", "#6c6c70"),
    claude: cssVariable("--cat-claude", "#d97757"),
    pi: cssVariable("--cat-pi", "#2f5f8a"),
    opencode: cssVariable("--cat-opencode", "#0f766e"),
    fontFamily: cssVariable(
      "--font-ui",
      '-apple-system, BlinkMacSystemFont, "Segoe UI Variable", "Segoe UI", sans-serif',
    ),
    text: cssVariable("--text-primary", "#18181c"),
    muted: cssVariable("--text-secondary", "#71717a"),
    border: cssVariable("--border-subtle", "#e4e4e7"),
    surface: cssVariable("--surface-raised", "#ffffff"),
  };
}

/**
 * 图表数据点的余量状态色：分档由 `quotaTone`（ADR-0017）拥有，这里只做
 * 分档 → 图表色值的映射。`ok` 档统一中性灰（不随服务色），与 QuotaProgress
 * 的 `--quota-tone` 同源。
 */
export function quotaChartColor(remainingPercent: number): string {
  switch (quotaTone(remainingPercent)) {
    case "warning":
      return cssVariable("--status-warning", "#f5a524");
    case "low":
      return cssVariable("--status-low", "#f3730e");
    case "danger":
      return cssVariable("--status-error", "#f31260");
    default:
      return cssVariable("--text-secondary", "#71717a");
  }
}
