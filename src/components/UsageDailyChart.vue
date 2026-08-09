<script setup lang="ts">
import { useResizeObserver } from "@vueuse/core";
import type { EChartsOption } from "echarts";
import { BarChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import { use } from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import VChart from "vue-echarts";

import type {
  UsageDashboardRange,
  UsageSource,
  UsageSummary,
  UsageSummaryRow,
} from "../features/usage/contracts";
import { formatCompactTokens, formatUsdNanos } from "../lib/format";
import { usageChartColors } from "../lib/chartTheme";

use([BarChart, GridComponent, TooltipComponent, CanvasRenderer]);

const props = defineProps<{
  day: Record<UsageSource, UsageSummary | null>;
  sources: readonly UsageSource[];
  range: UsageDashboardRange;
  chartRange: UsageDashboardRange;
  loaded: boolean;
  unavailable: boolean;
}>();

const { t, locale } = useI18n();
const chartRoot = ref<HTMLElement | null>(null);
const chart = ref<{ resize: () => void } | null>(null);
const themeVersion = ref(0);
let themeObserver: MutationObserver | null = null;

useResizeObserver(chartRoot, () => chart.value?.resize());

function localDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function dayKey(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function daysInRange(range: UsageDashboardRange): string[] {
  if (!range.from || !range.to) return [];

  const fromDate = localDay(new Date(range.from));
  const toDate = localDay(new Date(range.to));
  const values: string[] = [];

  for (
    let current = fromDate;
    current < toDate;
    current = new Date(current.getFullYear(), current.getMonth(), current.getDate() + 1)
  ) {
    values.push(dayKey(current));
  }

  return values;
}

const rowDates = computed(() => {
  const values = new Set<string>();
  for (const source of props.sources) {
    for (const row of props.day[source]?.rows ?? []) values.add(row.key);
  }
  return [...values];
});

const contextDates = computed(() => daysInRange(props.chartRange));
const selectedDates = computed(() => new Set(daysInRange(props.range)));
const contextual = computed(
  () => props.chartRange.from !== props.range.from || props.chartRange.to !== props.range.to,
);

const dates = computed(() => {
  const values = new Set([...contextDates.value, ...rowDates.value]);
  return [...values].sort();
});

const hasUsageRows = computed(() => rowDates.value.length > 0);

function dayRow(source: UsageSource, date: string): UsageSummaryRow | undefined {
  return props.day[source]?.rows.find((row) => row.key === date);
}

function dayTokens(source: UsageSource, date: string): number {
  return dayRow(source, date)?.tokens.totalTokens ?? 0;
}

function formatDay(value: string): string {
  const date = new Date(`${value}T00:00:00`);
  return new Intl.DateTimeFormat(locale.value, { day: "numeric", month: "numeric" }).format(date);
}

function barOpacity(date: string): number {
  return contextual.value && !selectedDates.value.has(date) ? 0.35 : 1;
}

interface TooltipParam {
  data: { dateKey?: string };
}

function tooltipFormatter(rawParams: unknown): string {
  const params = rawParams as TooltipParam[];
  const dateKey = params[0]?.data?.dateKey;
  if (!dateKey) return "";

  const colors = usageChartColors();
  const rowStyle =
    "display:flex;align-items:center;gap:6px;line-height:1.7;" + "font-family:" + colors.fontFamily;
  const numStyle = "font-variant-numeric:tabular-nums";
  const muted = `color:${colors.muted}`;

  let totalCostNanos = 0;
  let totalTokens = 0;
  for (const source of props.sources) {
    const row = dayRow(source, dateKey);
    if (!row) continue;
    totalCostNanos += row.cost.apiEquivalentCostNanos;
    totalTokens += row.tokens.totalTokens;
  }

  const lines = [
    `<div style="${rowStyle}">` +
      `<span style="flex:1">${t("main.grandTotal")}</span>` +
      `<span style="${numStyle};font-weight:600">${formatUsdNanos(locale.value, totalCostNanos)}</span>` +
      `<span style="${numStyle};min-inline-size:4.5em;text-align:right;font-weight:600">${formatCompactTokens(locale.value, totalTokens)}</span>` +
      `</div>`,
  ];

  for (const source of props.sources) {
    const row = dayRow(source, dateKey);
    if (!row) continue;
    lines.push(
      `<div style="${rowStyle}">` +
        `<span style="inline-size:8px;block-size:8px;border-radius:2px;background:${colors[source]};flex:none"></span>` +
        `<span style="flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${t(`provider.${source}`)}</span>` +
        `<span style="${numStyle}">${formatUsdNanos(locale.value, row.cost.apiEquivalentCostNanos)}</span>` +
        `<span style="${numStyle};min-inline-size:4.5em;text-align:right">${formatCompactTokens(locale.value, row.tokens.totalTokens)}</span>` +
        `</div>`,
    );
  }

  lines.push(`<div style="height:1px;background:${colors.border};margin:6px 0"></div>`);

  let totalInput = 0;
  let totalCacheRead = 0;
  let totalOutput = 0;
  for (const source of props.sources) {
    const row = dayRow(source, dateKey);
    if (!row) continue;
    totalInput += row.tokens.inputTokens;
    totalCacheRead += row.tokens.cacheReadInputTokens;
    totalOutput += row.tokens.outputTokens;
  }
  const hitRate = totalInput > 0 ? Math.round((totalCacheRead / totalInput) * 100) : 0;

  lines.push(
    `<div style="${rowStyle};color:${colors.muted};font-size:11px">` +
      `<span style="flex:1">${t("main.input")}</span><span style="${numStyle}">${formatCompactTokens(locale.value, totalInput)}</span>` +
      `</div>`,
    `<div style="${rowStyle};color:${colors.muted};font-size:11px">` +
      `<span style="flex:1">${t("main.output")}</span><span style="${numStyle}">${formatCompactTokens(locale.value, totalOutput)}</span>` +
      `</div>`,
    `<div style="${rowStyle};color:${colors.muted};font-size:11px">` +
      `<span style="flex:1">${t("main.cacheHit")}</span><span style="${numStyle}">${formatCompactTokens(locale.value, totalCacheRead)}</span>` +
      `</div>`,
    `<div style="${rowStyle};color:${colors.muted};font-size:11px">` +
      `<span style="flex:1">${t("main.cacheHitRate")}</span><span style="${numStyle}">${hitRate}%</span>` +
      `</div>`,
  );

  lines.push(
    `<div style="margin-top:6px;${muted};font-family:${colors.fontFamily}">${dateKey}</div>`,
  );

  return lines.join("");
}

const option = computed<EChartsOption>(() => {
  // 主题切换时通过 MutationObserver 改变依赖，重新读取 CSS variables。
  void themeVersion.value;
  const colors = usageChartColors();
  const categories = dates.value.map(formatDay);

  const sourceColors = props.sources.map((source) => colors[source]);
  return {
    animation: false,
    color: sourceColors,
    grid: { bottom: 24, containLabel: true, left: 8, right: 8, top: 10 },
    textStyle: { color: colors.text, fontFamily: colors.fontFamily },
    tooltip: {
      // 日期轴吸附到最近的类目，并关闭高频 mousemove 下的指针动画，
      // 避免光标在同一组柱体内轻微移动时 Tooltip 来回抖动。
      axisPointer: {
        animation: false,
        snap: true,
        triggerTooltip: true,
        type: "shadow",
      },
      backgroundColor: colors.surface,
      borderColor: colors.border,
      borderWidth: 1,
      confine: true,
      formatter: tooltipFormatter,
      padding: [8, 10],
      transitionDuration: 0,
      trigger: "axis",
    },
    xAxis: {
      axisLabel: {
        color: colors.muted,
        fontFamily: colors.fontFamily,
        fontSize: 9.5,
        hideOverlap: true,
        interval: Math.max(0, Math.ceil(categories.length / 5) - 1),
      },
      axisLine: { lineStyle: { color: colors.border } },
      axisTick: { show: false },
      data: categories,
      type: "category",
    },
    yAxis: {
      axisLabel: { show: false },
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { show: false },
      type: "value",
    },
    series: props.sources.map((source, index) => ({
      // 产物是纯 CSS 柱状：每列柱子占满列宽、上下堆叠两段、3px 圆角、降透明度。
      barMaxWidth: 18,
      barCategoryGap: "32%",
      data: dates.value.map((date) => ({
        dateKey: date,
        itemStyle: { opacity: barOpacity(date) },
        value: dayTokens(source, date),
      })),
      itemStyle: {
        color: colors[source],
        borderRadius: index === 0 ? [0, 0, 3, 3] : [3, 3, 0, 0],
      },
      name: t(`provider.${source}`),
      stack: "cost",
      type: "bar",
    })),
  };
});

onMounted(() => {
  themeObserver = new MutationObserver(() => {
    themeVersion.value += 1;
  });
  themeObserver.observe(document.documentElement, {
    attributeFilter: ["data-appearance"],
    attributes: true,
  });
});

onBeforeUnmount(() => {
  themeObserver?.disconnect();
  themeObserver = null;
});
</script>

<template>
  <div ref="chartRoot" class="usage-chart" role="img" :aria-label="t('a11y.usageChart')">
    <div v-if="!loaded || unavailable" class="usage-chart__empty">
      {{ unavailable ? t("main.unavailable") : t("main.loading") }}
    </div>
    <div v-else-if="!hasUsageRows" class="usage-chart__empty">{{ t("main.empty") }}</div>
    <VChart v-else ref="chart" class="usage-chart__canvas" :option="option" autoresize />
  </div>
</template>

<style scoped>
.usage-chart {
  min-block-size: 9.375rem;
  padding: 0.8125rem 0.875rem 0.5625rem;
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid var(--border-subtle);
  border-radius: 0.875rem;
  box-shadow: var(--usage-card-shadow, var(--shadow-lane));
}

.usage-chart__canvas {
  inline-size: 100%;
  block-size: 8.625rem;
}

.usage-chart__empty {
  display: grid;
  min-block-size: 8.625rem;
  place-items: center;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}
</style>
