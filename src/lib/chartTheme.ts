/**
 * ECharts 不会读取 CSS custom properties，因此主窗口图表只从这里取得主题色。
 * Provider 分类色与状态色保持分离，主题变化时重新计算 option 即可。
 */
export interface UsageChartColors {
  codex: string;
  claude: string;
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
    fontFamily: cssVariable(
      "--font-ui",
      '-apple-system, BlinkMacSystemFont, "Segoe UI Variable", "Segoe UI", sans-serif',
    ),
    text: cssVariable("--text-primary", "#11181c"),
    muted: cssVariable("--text-secondary", "#71717a"),
    border: cssVariable("--border-subtle", "#e4e4e7"),
    surface: cssVariable("--surface-raised", "#ffffff"),
  };
}
