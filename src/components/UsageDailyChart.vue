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

import type { UsageSource, UsageSummary } from "../features/usage/contracts";
import { formatCompactUsdNanos, formatUsdNanos } from "../lib/format";
import { usageChartColors } from "../lib/chartTheme";

use([BarChart, GridComponent, TooltipComponent, CanvasRenderer]);

const props = defineProps<{
  day: Record<UsageSource, UsageSummary | null>;
  loaded: boolean;
  unavailable: boolean;
}>();

const { t, locale } = useI18n();
const chartRoot = ref<HTMLElement | null>(null);
const chart = ref<{ resize: () => void } | null>(null);
const themeVersion = ref(0);
let themeObserver: MutationObserver | null = null;

useResizeObserver(chartRoot, () => chart.value?.resize());

const dates = computed(() => {
  const values = new Set<string>();
  for (const source of ["codex", "claude"] as const) {
    for (const row of props.day[source]?.rows ?? []) values.add(row.key);
  }
  return [...values].sort();
});

function costFor(source: UsageSource, date: string): number {
  return props.day[source]?.rows.find((row) => row.key === date)?.cost.apiEquivalentCostNanos ?? 0;
}

function formatDay(value: string): string {
  const date = new Date(`${value}T00:00:00`);
  return new Intl.DateTimeFormat(locale.value, { day: "numeric", month: "numeric" }).format(date);
}

const option = computed<EChartsOption>(() => {
  // 主题切换时通过 MutationObserver 改变依赖，重新读取 CSS variables。
  void themeVersion.value;
  const colors = usageChartColors();
  const categories = dates.value.map(formatDay);

  return {
    animation: false,
    color: [colors.codex, colors.claude],
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
      axisLabel: { color: colors.muted, fontFamily: colors.fontFamily, fontSize: 10 },
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
    series: [
      {
        barMaxWidth: 28,
        data: dates.value.map((date) => costFor("codex", date)),
        itemStyle: { color: colors.codex, borderRadius: [0, 0, 2, 2] },
        name: t("provider.codex"),
        stack: "cost",
        type: "bar",
      },
      {
        barMaxWidth: 28,
        data: dates.value.map((date) => costFor("claude", date)),
        itemStyle: { color: colors.claude, borderRadius: [2, 2, 0, 0] },
        name: t("provider.claude"),
        stack: "cost",
        type: "bar",
      },
    ],
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
    <div v-else-if="dates.length === 0" class="usage-chart__empty">{{ t("main.empty") }}</div>
    <VChart v-else ref="chart" class="usage-chart__canvas" :option="option" autoresize />
  </div>
</template>

<style scoped>
.usage-chart {
  min-block-size: 10.75rem;
  padding: 0.8125rem 0.875rem 0.5625rem;
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid var(--border-subtle);
  border-radius: 0.625rem;
  box-shadow: var(--usage-card-shadow, var(--shadow-lane));
}

.usage-chart__canvas {
  inline-size: 100%;
  block-size: 10rem;
}

.usage-chart__empty {
  display: grid;
  min-block-size: 10rem;
  place-items: center;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}
</style>
