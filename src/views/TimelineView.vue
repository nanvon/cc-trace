<script setup lang="ts">
/**
 * 主窗口额度历史（Timeline）视图。
 *
 * 基于 `usage.db` 的 `quota_events` 表：每个 Provider 只展示其**活动序列**（最新事件点
 * 所属的身份与窗口），旧身份／旧窗口的序列不展示，对应 cc-bar F-18 的「主账号镜像去重」。
 * 这里是历史读数，不是当前额度读数；不订阅额度事件，只读一次查询。
 */
import type { EChartsOption } from "echarts";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import { use } from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import VChart from "vue-echarts";

use([LineChart, GridComponent, TooltipComponent, CanvasRenderer]);

import { navigateMain } from "../features/app/navigation";
import { PROVIDER_ORDER, type ProviderId, type QuotaWindowKind } from "../features/quota/contracts";
import {
  activeSeriesByProvider,
  latestEvent,
  todayDelta,
  type QuotaSeries,
} from "../features/quota/history";
import { getQuotaHistory } from "../features/usage/api";
import type { QuotaHistoryEvent } from "../features/usage/contracts";
import { usageChartColors } from "../lib/chartTheme";

const { t, locale } = useI18n();
const router = useRouter();

const loading = ref(true);
const unavailable = ref(false);
const events = ref<QuotaHistoryEvent[]>([]);

const themeVersion = ref(0);
let themeObserver: MutationObserver | null = null;

const seriesByProvider = computed(() => {
  if (events.value.length === 0) return new Map();
  return activeSeriesByProvider(events.value);
});

const sections = computed(() =>
  PROVIDER_ORDER.map((provider) => {
    const series = seriesByProvider.value.get(provider);
    if (!series) return null;
    return { provider, series };
  }).filter((section): section is NonNullable<typeof section> => section !== null),
);

function windowLabel(kind: QuotaWindowKind, windowId: string | null): string {
  if (kind === "modelWeekly") {
    return t("quota.window.modelWeekly", { model: windowId ?? t("quota.window.unknown") });
  }
  return t(`quota.window.${kind}`);
}

function formatTime(value: string): string {
  return new Intl.DateTimeFormat(locale.value, {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function formatPercent(value: number): string {
  return new Intl.NumberFormat(locale.value, { maximumFractionDigits: 0 }).format(value);
}

function deltaText(series: QuotaSeries, now: Date): string {
  const delta = todayDelta(series, now);
  if (delta === null) return t("timeline.noChangeToday");
  const sign = delta > 0 ? "+" : "";
  return `${sign}${delta}%`;
}

function chartOption(series: QuotaSeries, provider: ProviderId): EChartsOption {
  void themeVersion.value;
  const colors = usageChartColors();
  const points = series.points;
  const providerColor = colors[provider];
  return {
    animation: false,
    color: [providerColor],
    grid: { bottom: 26, containLabel: true, left: 8, right: 8, top: 8 },
    textStyle: { color: colors.text, fontFamily: colors.fontFamily },
    tooltip: {
      axisPointer: { type: "line", animation: false },
      backgroundColor: colors.surface,
      borderColor: colors.border,
      borderWidth: 1,
      confine: true,
      formatter: (params: unknown) => {
        const data = params as { dataIndex: number };
        const point = points[data.dataIndex];
        return `${formatTime(point.observedAt)}<br/>${formatPercent(point.remainingPercent)}%`;
      },
      textStyle: { color: colors.text, fontFamily: colors.fontFamily, fontSize: 12 },
      transitionDuration: 0,
      trigger: "item",
    },
    xAxis: {
      axisLabel: {
        color: colors.muted,
        fontFamily: colors.fontFamily,
        fontSize: 10,
        hideOverlap: true,
      },
      axisLine: { lineStyle: { color: colors.border } },
      axisTick: { show: false },
      boundaryGap: false,
      data: points.map((point) => formatTime(point.observedAt)),
      type: "category",
    },
    yAxis: {
      axisLabel: {
        color: colors.muted,
        fontFamily: colors.fontFamily,
        fontSize: 10,
        formatter: (value: number) => `${formatPercent(value)}%`,
      },
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: colors.border, type: "solid" } },
      max: 100,
      min: 0,
      type: "value",
    },
    series: [
      {
        data: points.map((point) => [formatTime(point.observedAt), point.remainingPercent]),
        itemStyle: { color: providerColor },
        lineStyle: { color: providerColor, width: 2 },
        name: t("timeline.remaining"),
        showSymbol: points.length <= 40,
        type: "line",
      },
    ],
  };
}

function backToUsage(): void {
  void navigateMain(router, "quota", "usage-title");
}

onMounted(async () => {
  themeObserver = new MutationObserver(() => {
    themeVersion.value += 1;
  });
  themeObserver.observe(document.documentElement, {
    attributeFilter: ["data-appearance"],
    attributes: true,
  });

  try {
    const result = await getQuotaHistory({
      provider: null,
      from: null,
      to: null,
      limit: 500,
    });
    events.value = result.events;
  } catch {
    unavailable.value = true;
  } finally {
    loading.value = false;
  }
});

onBeforeUnmount(() => {
  themeObserver?.disconnect();
  themeObserver = null;
});
</script>

<template>
  <main class="timeline" :aria-label="t('a11y.timelineRegion')">
    <div class="timeline__inner">
      <header class="timeline__header">
        <button type="button" class="button button--quiet timeline__back" @click="backToUsage">
          <span aria-hidden="true">←</span>
          {{ t("timeline.backToUsage") }}
        </button>
        <h1 id="timeline-title" tabindex="-1">{{ t("timeline.title") }}</h1>
      </header>

      <p v-if="unavailable" class="timeline__notice">{{ t("timeline.unavailable") }}</p>
      <p v-else-if="loading" class="timeline__notice">{{ t("timeline.loading") }}</p>
      <p v-else-if="sections.length === 0" class="timeline__notice">{{ t("timeline.empty") }}</p>

      <section
        v-for="section in sections"
        :key="section.provider"
        class="timeline__section"
        :data-provider="section.provider"
        :aria-label="t(`provider.${section.provider}`)"
      >
        <div class="timeline__section-head">
          <h2>
            <span class="timeline__dot" aria-hidden="true"></span>
            {{ t(`provider.${section.provider}`) }}
            <small>{{ windowLabel(section.series.windowKind, section.series.windowId) }}</small>
          </h2>
          <dl class="timeline__kpis">
            <div>
              <dt>{{ t("timeline.current") }}</dt>
              <dd class="numeric">
                {{ formatPercent(latestEvent(section.series).remainingPercent) }}%
              </dd>
            </div>
            <div>
              <dt>{{ t("timeline.todayDelta") }}</dt>
              <dd class="numeric">{{ deltaText(section.series, new Date()) }}</dd>
            </div>
            <div>
              <dt>{{ t("timeline.events") }}</dt>
              <dd class="numeric">{{ section.series.points.length }}</dd>
            </div>
          </dl>
        </div>

        <div class="timeline__body">
          <div
            class="timeline__chart"
            role="img"
            :aria-label="t('a11y.timelineChart', { provider: t(`provider.${section.provider}`) })"
          >
            <VChart
              class="timeline__canvas"
              :option="chartOption(section.series, section.provider)"
              autoresize
            />
          </div>

          <div class="timeline__table-wrap">
            <table class="timeline__table">
              <caption class="visually-hidden">
                {{
                  t("a11y.timelineTable", { provider: t(`provider.${section.provider}`) })
                }}
              </caption>
              <thead>
                <tr>
                  <th scope="col">{{ t("timeline.observedAt") }}</th>
                  <th scope="col">{{ t("timeline.remaining") }}</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="point in [...section.series.points].reverse()"
                  :key="`${point.windowId ?? ''}-${point.observedAt}`"
                >
                  <td class="numeric">{{ formatTime(point.observedAt) }}</td>
                  <td class="numeric">{{ formatPercent(point.remainingPercent) }}%</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </section>
    </div>
  </main>
</template>

<style scoped>
.timeline {
  --usage-canvas: color-mix(in srgb, var(--surface-primary) 86%, var(--border-subtle) 14%);
  --usage-surface: var(--surface-raised);
  --usage-divider: var(--border-subtle);
  --usage-card-shadow: var(--shadow-lane);
  min-block-size: 100vh;
  padding: clamp(1.125rem, 3vw, 1.375rem) clamp(1.125rem, 3vw, 1.875rem) 2.125rem;
  background: var(--usage-canvas);
  font-family: var(--font-ui);
}

.timeline__inner {
  inline-size: min(100%, 75rem);
  margin-inline: auto;
}

.timeline__header {
  display: flex;
  align-items: baseline;
  gap: var(--space-4);
  padding-block-end: 0.625rem;
  border-block-end: 1px solid var(--usage-divider);
  margin-block-end: 1rem;
}

.timeline__header h1 {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 680;
  letter-spacing: -0.025em;
  line-height: 1.15;
}

.timeline__back {
  min-inline-size: 3.25rem;
  min-block-size: 2.5rem;
  padding-inline: 0.75rem;
  border-radius: var(--radius-control);
  font-size: 0.75rem;
}

.timeline__notice {
  padding: 2.5rem 1rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

.timeline__section {
  margin-block-end: 1.25rem;
  padding: 0.9375rem 1rem 1rem;
  background: var(--usage-surface);
  border: 1px solid var(--border-subtle);
  border-radius: 0.625rem;
  box-shadow: var(--usage-card-shadow);
}

.timeline__section-head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 1rem;
  margin-block-end: 0.875rem;
}

.timeline__section-head h2 {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  margin: 0;
  font-size: 0.875rem;
  font-weight: 680;
}

.timeline__section-head h2 small {
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 550;
}

.timeline__dot {
  inline-size: 0.4375rem;
  block-size: 0.4375rem;
  flex: 0 0 auto;
  border-radius: 0.125rem;
  background: var(--cat-codex);
}

.timeline__section[data-provider="claude"] .timeline__dot {
  background: var(--cat-claude);
}

.timeline__kpis {
  display: flex;
  gap: 1.25rem;
  margin: 0;
}

.timeline__kpis div {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
}

.timeline__kpis dt {
  color: var(--text-secondary);
  font-size: 0.59375rem;
}

.timeline__kpis dd {
  margin: 0.125rem 0 0;
  font-size: 0.8125rem;
  font-weight: 650;
}

.timeline__body {
  display: grid;
  grid-template-columns: minmax(0, 2fr) minmax(0, 1fr);
  gap: 1rem;
}

.timeline__chart {
  min-inline-size: 0;
  block-size: 11rem;
}

.timeline__canvas {
  inline-size: 100%;
  block-size: 100%;
}

.timeline__table-wrap {
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: 0.5rem;
}

.timeline__table {
  inline-size: 100%;
  border-collapse: collapse;
  background: var(--usage-surface);
}

.timeline__table th,
.timeline__table td {
  block-size: 2rem;
  padding: 0 0.625rem;
  border-block-end: 1px solid var(--border-subtle);
  font-size: 0.65625rem;
  font-variant-numeric: tabular-nums;
  text-align: right;
  white-space: nowrap;
}

.timeline__table thead th {
  color: var(--text-secondary);
  font-size: 0.59375rem;
  font-weight: 550;
  text-align: left;
}

.timeline__table td {
  font-weight: 500;
}

@media (max-width: 760px) {
  .timeline__section-head {
    align-items: flex-start;
    flex-direction: column;
  }

  .timeline__body {
    grid-template-columns: 1fr;
  }
}
</style>
