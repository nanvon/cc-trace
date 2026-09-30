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
  UsageGranularity,
  UsageSource,
  UsageSummary,
} from "../features/usage/contracts";
import {
  barWidthForCount,
  buildPeriodSamples,
  type PeriodSample,
} from "../features/usage/overview";
import { usageBucketStart, usageChartUsesContext, usageDayKey } from "../features/usage/ranges";
import { formatCompactTokens, formatUsdNanos } from "../lib/format";
import { usageChartColors } from "../lib/chartTheme";

use([BarChart, GridComponent, TooltipComponent, CanvasRenderer]);

const props = defineProps<{
  day: Record<UsageSource, UsageSummary | null>;
  sources: readonly UsageSource[];
  range: UsageDashboardRange;
  chartRange: UsageDashboardRange;
  granularity: UsageGranularity;
  loaded: boolean;
  unavailable: boolean;
}>();

const { t, locale } = useI18n();
const chartRoot = ref<HTMLElement | null>(null);
const chart = ref<{ resize: () => void } | null>(null);
const themeVersion = ref(0);
let themeObserver: MutationObserver | null = null;

useResizeObserver(chartRoot, () => chart.value?.resize());

function nextPeriod(start: Date): Date {
  if (props.granularity === "week") {
    return new Date(start.getFullYear(), start.getMonth(), start.getDate() + 7);
  }
  if (props.granularity === "month") {
    return new Date(start.getFullYear(), start.getMonth() + 1, 1);
  }
  return new Date(start.getFullYear(), start.getMonth(), start.getDate() + 1);
}

/** 图表窗口内的全部周期起点（无用量的周期补零，柱子不塌缩）。全部范围只显示有数据的周期。 */
function periodsInRange(range: UsageDashboardRange): string[] {
  if (!range.from || !range.to) return [];
  const end = new Date(range.to);
  const values: string[] = [];
  for (
    let current = usageBucketStart(new Date(range.from), props.granularity);
    current < end;
    current = nextPeriod(current)
  ) {
    values.push(usageDayKey(current));
  }
  return values;
}

const samples = computed(() => buildPeriodSamples(props.day, props.sources, props.granularity));
const sampleByKey = computed(() => new Map(samples.value.map((sample) => [sample.key, sample])));

const dates = computed(() => {
  const values = new Set([
    ...periodsInRange(props.chartRange),
    ...samples.value.map((sample) => sample.key),
  ]);
  return [...values].sort();
});

const hasUsageRows = computed(() => samples.value.length > 0);
const contextual = computed(() => usageChartUsesContext(props.range, props.granularity));
const highlightedKey = computed(() =>
  contextual.value && props.range.from
    ? usageDayKey(usageBucketStart(new Date(props.range.from), props.granularity))
    : null,
);

function sourceTokens(sample: PeriodSample | undefined, source: UsageSource): number {
  return sample?.bySource[source]?.tokens.totalTokens ?? 0;
}

function parseKey(value: string): Date {
  return new Date(`${value}T00:00:00`);
}

function formatAxis(value: string): string {
  const date = parseKey(value);
  if (props.granularity === "month") {
    return new Intl.DateTimeFormat(locale.value, { month: "short", year: "2-digit" }).format(date);
  }
  return new Intl.DateTimeFormat(locale.value, { day: "numeric", month: "numeric" }).format(date);
}

/** 悬浮标题：日 `2026-09-03`，周 `2026-09-01 – 09-07`，月 `2026-09`。 */
function periodLabel(value: string): string {
  if (props.granularity === "month") return value.slice(0, 7);
  if (props.granularity === "week") {
    const start = parseKey(value);
    const end = new Date(start.getFullYear(), start.getMonth(), start.getDate() + 6);
    return `${value} – ${usageDayKey(end).slice(5)}`;
  }
  return value;
}

function barOpacity(date: string): number {
  return highlightedKey.value !== null && date !== highlightedKey.value ? 0.35 : 1;
}

interface TooltipParam {
  data: { dateKey?: string };
}

function tooltipFormatter(rawParams: unknown): string {
  const params = rawParams as TooltipParam[];
  const dateKey = params[0]?.data?.dateKey;
  if (!dateKey) return "";
  const sample = sampleByKey.value.get(dateKey);

  const colors = usageChartColors();
  const rowStyle =
    "display:flex;align-items:center;gap:6px;line-height:1.7;" + "font-family:" + colors.fontFamily;
  const numStyle = "font-variant-numeric:tabular-nums";
  const muted = `color:${colors.muted}`;

  const lines = [
    `<div style="${rowStyle}">` +
      `<span style="flex:1">${t("main.grandTotal")}</span>` +
      `<span style="${numStyle};font-weight:600">${formatUsdNanos(locale.value, sample?.total.cost.apiEquivalentCostNanos ?? 0)}</span>` +
      `<span style="${numStyle};min-inline-size:4.5em;text-align:right;font-weight:600">${formatCompactTokens(locale.value, sample?.total.tokens.totalTokens ?? 0)}</span>` +
      `</div>`,
  ];

  for (const source of props.sources) {
    const row = sample?.bySource[source];
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

  const totalInput = sample?.total.tokens.inputTokens ?? 0;
  const totalCacheRead = sample?.total.tokens.cacheReadInputTokens ?? 0;
  const totalOutput = sample?.total.tokens.outputTokens ?? 0;
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
    `<div style="margin-top:6px;${muted};font-family:${colors.fontFamily}">${periodLabel(dateKey)}</div>`,
  );

  return lines.join("");
}

const option = computed<EChartsOption>(() => {
  // 主题切换时通过 MutationObserver 改变依赖，重新读取 CSS variables。
  void themeVersion.value;
  const colors = usageChartColors();
  const categories = dates.value.map(formatAxis);

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
        fontSize: 10.5,
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
      // 柱宽按样本数分档（30 根为 10），不随面板宽度拉伸。
      barWidth: barWidthForCount(dates.value.length),
      data: dates.value.map((date) => ({
        dateKey: date,
        itemStyle: { opacity: barOpacity(date) },
        value: sourceTokens(sampleByKey.value.get(date), source),
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
  min-block-size: 12.5rem;
  padding: 0.8125rem 0.875rem 0.5625rem;
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
}

.usage-chart__canvas {
  inline-size: 100%;
  block-size: 12.5rem;
}

.usage-chart__empty {
  display: grid;
  min-block-size: 12.5rem;
  place-items: center;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}
</style>
