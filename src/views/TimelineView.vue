<script setup lang="ts">
/**
 * 主窗口额度历史（Timeline）视图。
 *
 * 基于 `usage.db` 的 `quota_events` 表：每个 Provider 只展示其**活动序列**（最新事件点
 * 所属的身份与窗口），旧身份／旧窗口的序列不展示，对应 cc-bar F-18 的「主账号镜像去重」。
 * 这里是历史读数，不是当前额度读数；不订阅额度事件，只读一次查询。
 *
 * 功能与信息层级对齐 cc-bar `QuotaTimelineAccountPanel`：面板 header 的
 * 窗口徽标（5H／WK／MODEL／CURRENT）与 Current／Today／Latest 三指标、
 * 状态色数据点、Y 轴四档边界刻度、四列事件表（Time／Change／After／Reset）。
 * 视觉收敛进主窗口现行语法（贴合式弱化卡片、hairline 描边、radius-medium）。
 */
import type { EChartsOption } from "echarts";
import { LineChart } from "echarts/charts";
import { GridComponent, TooltipComponent } from "echarts/components";
import { use } from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import VChart from "vue-echarts";

use([LineChart, GridComponent, TooltipComponent, CanvasRenderer]);

import { PROVIDER_ORDER, type ProviderId, type QuotaWindowKind } from "../features/quota/contracts";
import {
  activeSeriesByProvider,
  eventRows,
  latestEvent,
  todayDelta,
  type QuotaSeries,
} from "../features/quota/history";
import { getQuotaHistory } from "../features/usage/api";
import type { QuotaHistoryEvent } from "../features/usage/contracts";
import { quotaChartColor, usageChartColors } from "../lib/chartTheme";
import { quotaTone } from "../lib/quotaTone";

const { t, locale } = useI18n();

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
    return { provider, series, rows: [...eventRows(series)].reverse() };
  }).filter((section): section is NonNullable<typeof section> => section !== null),
);

/** 窗口徽标短标签：固定不翻译（对齐 cc-bar `limitKindLabel`）。 */
function windowBadge(kind: QuotaWindowKind): string {
  switch (kind) {
    case "fiveHour":
      return "5H";
    case "weekly":
      return "WK";
    case "modelWeekly":
      return "MODEL";
    default:
      return "CURRENT";
  }
}

/** 只有 `modelWeekly` 需要把模型名带出来（徽标无法表达）；其余窗口由徽标承担。 */
function windowHint(kind: QuotaWindowKind, windowId: string | null): string | null {
  if (kind !== "modelWeekly") return null;
  return windowId ?? t("quota.window.unknown");
}

function formatDate(date: Date): string {
  const year = date.getFullYear();
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function formatTime(value: string): string {
  return new Intl.DateTimeFormat(locale.value, {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function formatClock(value: string): string {
  return new Intl.DateTimeFormat(locale.value, {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function formatPercent(value: number): string {
  return new Intl.NumberFormat(locale.value, { maximumFractionDigits: 0 }).format(value);
}

/** Today 指标：当日净变化；当天没有事件点时为不可得（`—`）。 */
function deltaText(series: QuotaSeries, now: Date): string {
  const delta = todayDelta(series, now);
  if (delta === null) return "—";
  return `${delta > 0 ? "+" : ""}${delta}%`;
}

/** Change 列：相对前一点的整数差；第一点无前值。 */
function changeText(delta: number | null): string {
  if (delta === null) return "—";
  return `${delta > 0 ? "+" : ""}${delta}%`;
}

/** After 列的余量分档（ok 档中性，与图表数据点同一分档源）。 */
function toneClass(remainingPercent: number): string {
  return `timeline__tone-${quotaTone(remainingPercent)}`;
}

function chartOption(series: QuotaSeries, provider: ProviderId): EChartsOption {
  void themeVersion.value;
  const colors = usageChartColors();
  const points = series.points;
  const providerColor = colors[provider];
  const xInterval = points.length > 5 ? Math.ceil(points.length / 5) : 0;
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
        interval: xInterval,
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
        /* 只显示四档分档边界刻度（对齐 cc-bar `[0, 20, 50, 80, 100]`） */
        interval: (value: number) => [0, 20, 50, 80, 100].includes(value),
      },
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { interval: 20, lineStyle: { color: colors.border, type: "solid" } },
      max: 100,
      min: 0,
      type: "value",
    },
    series: [
      {
        data: points.map((point) => ({
          value: [formatTime(point.observedAt), point.remainingPercent],
          itemStyle: { color: quotaChartColor(point.remainingPercent) },
        })),
        itemStyle: { color: providerColor },
        lineStyle: { color: providerColor, width: 2 },
        name: t("timeline.remaining"),
        showSymbol: points.length <= 40,
        symbol: "circle",
        symbolSize: 7,
        type: "line",
      },
    ],
  };
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
        <div class="timeline__heading">
          <h1 id="timeline-title" tabindex="-1">{{ t("timeline.title") }}</h1>
          <p class="timeline__subtitle">{{ t("timeline.subtitle") }}</p>
        </div>
        <span class="timeline__date" aria-hidden="true">{{ formatDate(new Date()) }}</span>
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
            <span class="timeline__badge">{{ windowBadge(section.series.windowKind) }}</span>
            <small v-if="windowHint(section.series.windowKind, section.series.windowId)">
              {{ windowHint(section.series.windowKind, section.series.windowId) }}
            </small>
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
              <dt>{{ t("timeline.latest") }}</dt>
              <dd class="numeric">{{ formatTime(latestEvent(section.series).observedAt) }}</dd>
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
            <div
              class="timeline__table-scroll"
              :class="{ 'timeline__table-scroll--limit': section.rows.length > 8 }"
            >
              <table class="timeline__table">
                <caption class="visually-hidden">
                  {{
                    t("a11y.timelineTable", { provider: t(`provider.${section.provider}`) })
                  }}
                </caption>
                <thead>
                  <tr>
                    <th scope="col">{{ t("timeline.observedAt") }}</th>
                    <th scope="col">{{ t("timeline.change") }}</th>
                    <th scope="col">{{ t("timeline.after") }}</th>
                    <th scope="col">{{ t("timeline.reset") }}</th>
                  </tr>
                </thead>
                <tbody>
                  <tr
                    v-for="row in section.rows"
                    :key="`${row.event.windowId ?? ''}-${row.event.observedAt}`"
                  >
                    <td class="numeric">{{ formatTime(row.event.observedAt) }}</td>
                    <td
                      class="numeric"
                      :data-direction="
                        row.deltaPercent !== null && row.deltaPercent < 0 ? 'down' : 'up'
                      "
                      :data-change="row.deltaPercent !== null ? 'true' : undefined"
                    >
                      {{ changeText(row.deltaPercent) }}
                    </td>
                    <td class="numeric" :class="toneClass(row.event.remainingPercent)">
                      {{ formatPercent(row.event.remainingPercent) }}%
                    </td>
                    <td class="numeric">
                      {{ row.event.resetsAt ? formatClock(row.event.resetsAt) : "—" }}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </section>
    </div>
  </main>
</template>

<style scoped>
.timeline {
  --usage-canvas: var(--surface-primary);
  --usage-surface: var(--surface-raised);
  --usage-divider: var(--border-subtle);
  container-type: inline-size;
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
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--space-4);
  padding-block-end: 0.625rem;
  border-block-end: 1px solid var(--usage-divider);
  margin-block-end: 1rem;
}

.timeline__heading {
  display: flex;
  flex-direction: column;
  gap: 0.125rem;
  min-inline-size: 0;
}

.timeline__header h1 {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 680;
  letter-spacing: -0.025em;
  line-height: 1.15;
}

.timeline__header h1[tabindex="-1"]:focus {
  outline: none;
}

.timeline__subtitle {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.timeline__date {
  flex: 0 0 auto;
  color: var(--text-secondary);
  font-family: var(--font-data);
  font-size: 0.6875rem;
  font-variant-numeric: tabular-nums;
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
  border: 1px solid var(--border-hairline);
  border-radius: var(--radius-medium);
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
  min-inline-size: 0;
  margin: 0;
  font-size: 0.875rem;
  font-weight: 680;
  white-space: nowrap;
}

.timeline__section-head h2 small {
  overflow: hidden;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 550;
  text-overflow: ellipsis;
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

/* 窗口徽标：中性胶囊，语法对齐对话页速度徽标（不占交互色） */
.timeline__badge {
  padding: 0.0625rem 0.375rem;
  border-radius: 999px;
  background: color-mix(in srgb, var(--text-secondary) 12%, transparent);
  color: var(--text-secondary);
  font-size: 0.65625rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.timeline__kpis {
  display: flex;
  gap: 1.25rem;
  flex: 0 0 auto;
  margin: 0;
}

.timeline__kpis div {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
}

.timeline__kpis dt {
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.timeline__kpis dd {
  margin: 0.125rem 0 0;
  font-size: 0.8125rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
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

/* 表体内部滚动（对齐 cc-bar maxVisibleRows=8）：行数不超过 8 时自然高度，超过后固定高度 */
.timeline__table-scroll {
  overflow-y: auto;
  overscroll-behavior: contain;
}

.timeline__table-scroll--limit {
  max-block-size: calc(8 * 2.0625rem);
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
  border-block-end: 1px solid color-mix(in srgb, var(--border-subtle) 55%, transparent);
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
  text-align: right;
  white-space: nowrap;
}

.timeline__table thead th {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--usage-surface);
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 600;
  letter-spacing: 0.02em;
  text-align: left;
  border-block-end-color: var(--border-subtle);
}

.timeline__table td:first-child,
.timeline__table thead th:first-child {
  text-align: left;
}

/* Change 列：红涨绿跌（对齐主窗口 KPI delta 与 cc-bar 表内着色） */
.timeline__table td[data-change="true"] {
  font-weight: 600;
}

.timeline__table td[data-direction="up"] {
  color: var(--status-success);
}

.timeline__table td[data-direction="down"] {
  color: var(--status-error);
}

/* After 列：余量四档分档（ok 档中性灰，不随服务色） */
.timeline__tone-ok {
  color: var(--text-secondary);
}

.timeline__tone-warning {
  color: var(--status-warning);
}

.timeline__tone-low {
  color: var(--status-low);
}

.timeline__tone-danger {
  color: var(--status-error);
}

@container (max-width: 760px) {
  .timeline__section-head {
    align-items: flex-start;
    flex-direction: column;
  }

  .timeline__body {
    grid-template-columns: 1fr;
  }
}
</style>
