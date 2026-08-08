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

import type { UsageDashboardRange, UsageSource, UsageSummary } from "../features/usage/contracts";
import { formatCompactUsdNanos, formatUsdNanos } from "../lib/format";
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

function costFor(source: UsageSource, date: string): number {
  return props.day[source]?.rows.find((row) => row.key === date)?.cost.apiEquivalentCostNanos ?? 0;
}

function formatDay(value: string): string {
  const date = new Date(`${value}T00:00:00`);
  return new Intl.DateTimeFormat(locale.value, { day: "numeric", month: "numeric" }).format(date);
}

function barOpacity(date: string): number {
  return contextual.value && !selectedDates.value.has(date) ? 0.35 : 1;
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
      textStyle: { color: colors.text, fontFamily: colors.fontFamily, fontSize: 12 },
      trigger: "axis",
      transitionDuration: 0,
      valueFormatter: (value) => formatUsdNanos(locale.value, Number(value)),
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
      axisLabel: {
        color: colors.muted,
        fontFamily: colors.fontFamily,
        fontSize: 10,
        formatter: (value: number) => formatCompactUsdNanos(locale.value, value),
      },
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: colors.border, type: "solid" } },
      type: "value",
    },
    series: props.sources.map((source, index) => ({
      // 产物是纯 CSS 柱状：每列柱子占满列宽、上下堆叠两段、3px 圆角、降透明度。
      barMaxWidth: 18,
      barCategoryGap: "32%",
      data: dates.value.map((date) => ({
        itemStyle: { opacity: barOpacity(date) },
        value: costFor(source, date),
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
