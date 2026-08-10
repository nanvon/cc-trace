<script setup lang="ts">
/**
 * 主窗口本地用量页。
 *
 * 页面只读取 Rust 本地用量摘要，不读取额度，也不把 Conversations、设置等后续能力
 * 提前塞进主窗口。布局基线来自 `prototypes/usage-page/index.html` 与 ADR-0020；
 * 顶栏（右上 range 分段＋扫描状态＋刷新）、KPI delta、Token 拆分与 Fast 汇总对齐 cc-bar。
 */
import { DatePicker as VDatePicker } from "v-calendar";
import "v-calendar/style.css";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";

import UsageDailyChart from "../components/UsageDailyChart.vue";
import UsageModelTable from "../components/UsageModelTable.vue";
import UsageProviderCard from "../components/UsageProviderCard.vue";
import { navigateMain } from "../features/app/navigation";
import type { UsageDashboardRange, UsageSource } from "../features/usage/contracts";
import {
  customUsageRange,
  usageChartRange,
  usageDashboardRanges,
  usageDatePickerRange,
  usageRangePresets,
  type UsageRangePreset,
} from "../features/usage/ranges";
import { useUsageStore } from "../features/usage/store";
import { formatUsageCost, presentUsageTokens } from "../features/usage/presentation";

const { t, locale } = useI18n();
const router = useRouter();
const usage = useUsageStore();

const presets = usageRangePresets();
const providerSources = computed(() => usage.dashboardSources);
const currentPreset = ref<UsageDashboardRange["preset"]>("today");
const initialRange = usageDashboardRanges().today;
const selectedRange = ref<UsageDashboardRange>(initialRange);
type DateRangeInput = { start: Date; end: Date } | null;

function dateRangeInputValue(range: UsageDashboardRange): DateRangeInput {
  const dates = usageDatePickerRange(range);
  return dates ? { start: dates[0], end: dates[1] } : null;
}

const customDates = ref<DateRangeInput>(dateRangeInputValue(initialRange));
const calendarLocale = computed(() =>
  locale.value.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US",
);
const todayDate = computed(() => {
  const today = new Date();
  return new Date(today.getFullYear(), today.getMonth(), today.getDate());
});
const chartRange = computed(() => usageChartRange(selectedRange.value));
const chartUsesContextWindow = computed(
  () =>
    chartRange.value.from !== selectedRange.value.from ||
    chartRange.value.to !== selectedRange.value.to,
);

const sourceSummary = computed(() => usage.visibleSourceSummary);
const allServicesOff = computed(() => usage.visibleSources.length === 0);
const dashboardReady = computed(() => usage.dashboardLoaded && !usage.dashboardLoading);
const providerTotalTokens = computed(() => {
  if (!dashboardReady.value || usage.dashboardUnavailable || !sourceSummary.value) return 0;
  return sourceSummary.value.tokens.totalTokens;
});

/** 前区间等长对比；无 previous（all/custom）或上一期无数据时返回 null。 */
function deltaPercent(current: number, previous: number | null | undefined): number | null {
  if (previous === null || previous === undefined || previous <= 0) return null;
  if (current === previous) return 0;
  return ((current - previous) / previous) * 100;
}

interface UsageKpiCard {
  key: string;
  label: string;
  text: string;
  unit?: string;
  provider?: UsageSource;
  delta?: number | null;
}

const kpiCards = computed<UsageKpiCard[]>(() => {
  const ready = dashboardReady.value && !usage.dashboardUnavailable;
  const noValue = t("main.noValue");
  const tokens = totalTokens.value;
  const cost = totalCost.value;
  const previousSummary = usage.dashboardPrevious;
  const providerCost = (source: UsageSource): string => {
    const row = sourceSummary.value?.rows.find((candidate) => candidate.key === source);
    if (!ready || !row || row.entryCount === 0) return noValue;
    return (
      formatUsageCost(locale.value, row.cost, row.entryCount, t("main.lessThanCent")) ?? noValue
    );
  };
  const previousProviderCost = (source: UsageSource): number | null => {
    const row = previousSummary?.rows.find((candidate) => candidate.key === source);
    if (!row || row.entryCount === 0) return null;
    return row.cost.apiEquivalentCostNanos;
  };
  return [
    {
      key: "total-tokens",
      label: t("main.totalTokens"),
      text: tokens?.value ?? noValue,
      ...(tokens?.unit ? { unit: tokens.unit } : {}),
      delta: deltaPercent(
        sourceSummary.value?.tokens.totalTokens ?? 0,
        previousSummary?.tokens.totalTokens,
      ),
    },
    {
      key: "total-cost",
      label: t("main.totalCost"),
      text: cost ?? noValue,
      delta: deltaPercent(
        sourceSummary.value?.cost.apiEquivalentCostNanos ?? 0,
        previousSummary?.cost.apiEquivalentCostNanos,
      ),
    },
    ...providerSources.value.map((source) => ({
      key: `provider-${source}`,
      label: t(`provider.${source}`),
      text: providerCost(source),
      provider: source,
      delta: deltaPercent(
        sourceSummary.value?.rows.find((candidate) => candidate.key === source)?.cost
          .apiEquivalentCostNanos ?? 0,
        previousProviderCost(source),
      ),
    })),
  ];
});
const tokenUnitSeparator = computed(() => (locale.value.toLowerCase().startsWith("zh") ? "" : " "));
const totalTokens = computed(() => {
  if (
    !dashboardReady.value ||
    usage.dashboardUnavailable ||
    !sourceSummary.value ||
    sourceSummary.value.entryCount === 0
  ) {
    return null;
  }
  return presentUsageTokens(locale.value, sourceSummary.value.tokens.totalTokens);
});
const totalCost = computed(() => {
  if (!dashboardReady.value || usage.dashboardUnavailable || !sourceSummary.value) return null;
  return formatUsageCost(
    locale.value,
    sourceSummary.value.cost,
    sourceSummary.value.entryCount,
    t("main.lessThanCent"),
  );
});

/** Token 拆分面板：堆叠条三段与命中率；数据同 KPI 口径（可见源）。 */
const breakdown = computed(() => {
  const summary = sourceSummary.value;
  const ready = dashboardReady.value && !usage.dashboardUnavailable && !!summary;
  const present = (value: number) => {
    if (!ready) return t("main.noValue");
    const display = presentUsageTokens(locale.value, value);
    return display.unit ? `${display.value}${display.unit}` : display.value;
  };
  const tokens = summary?.tokens;
  const total = tokens?.totalTokens ?? 0;
  const input = tokens?.inputTokens ?? 0;
  const output = tokens?.outputTokens ?? 0;
  const cacheRead = tokens?.cacheReadInputTokens ?? 0;
  const hitRate = ready && total > 0 ? Math.round((cacheRead / total) * 100) : null;
  const fast = summary?.fast;
  const fastTotal = fast?.rawTokens ?? 0;
  const fastShare =
    ready && fastTotal > 0 && total > 0 ? Math.round((fastTotal / total) * 100) : null;
  const multiplier =
    fast && fast.minimumMultiplier
      ? fast.maximumMultiplier && fast.maximumMultiplier !== fast.minimumMultiplier
        ? `${fast.minimumMultiplier}–${fast.maximumMultiplier}`
        : fast.minimumMultiplier
      : null;
  return {
    ready: Boolean(ready),
    total,
    totalText: present(total),
    input: present(input),
    output: present(output),
    cacheRead: present(cacheRead),
    hitRate,
    fastTotal: present(fastTotal),
    billingEquivalent: fast?.billingEquivalentTokens ?? "0",
    multiplier,
    hasFast: ready && fastTotal > 0,
    fastShare,
    segments: [
      { tokens: input, opacity: "0.85" },
      { tokens: output, opacity: "0.6" },
      { tokens: cacheRead, opacity: "0.4" },
    ],
  };
});

const scanText = computed(() => {
  if (usage.scanning) return t("main.scanningUsage");
  const finishedAt = usage.status?.finishedAt;
  if (!finishedAt) return t("main.neverScanned");
  return t("main.lastScan", { time: formatDateTime(finishedAt) });
});

const liveMessage = computed(() => {
  if (allServicesOff.value) return t("main.allServicesOff");
  if (usage.dashboardLoading) return t("main.loading");
  if (usage.dashboardUnavailable) return t("main.unavailable");
  if (usage.partial) return t("main.partial");
  return "";
});

let refreshPoll: ReturnType<typeof setInterval> | null = null;

/** 手动刷新：启动增量扫描并轮询，结束后重载当前范围。 */
async function refreshUsage(): Promise<void> {
  if (usage.scanning) return;
  const started = await usage.startScan();
  if (!started) return;
  if (refreshPoll) clearInterval(refreshPoll);
  refreshPoll = setInterval(async () => {
    const stillScanning = await usage.poll();
    if (!stillScanning) {
      if (refreshPoll) {
        clearInterval(refreshPoll);
        refreshPoll = null;
      }
      await usage.loadDashboard(usage.dashboardRange);
    }
  }, 2000);
}

function formatDateTime(value: string): string {
  return new Intl.DateTimeFormat(locale.value, {
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    month: "numeric",
  }).format(new Date(value));
}

function formatDateRangeInput(start?: string, end?: string): string {
  return [start, end].filter(Boolean).join(" – ");
}

function selectPreset(preset: UsageRangePreset): void {
  const range = usageDashboardRanges()[preset];
  currentPreset.value = preset;
  customDates.value = dateRangeInputValue(range);
  selectedRange.value = range;
  void usage.loadDashboard(selectedRange.value);
}

function sameDate(left: Date, right: Date): boolean {
  return (
    left.getFullYear() === right.getFullYear() &&
    left.getMonth() === right.getMonth() &&
    left.getDate() === right.getDate()
  );
}

function handleCustomRange(value: DateRangeInput): void {
  if (!value?.start || !value.end) return;
  const from = new Date(value.start.getFullYear(), value.start.getMonth(), value.start.getDate());
  const to = new Date(value.end.getFullYear(), value.end.getMonth(), value.end.getDate());

  const normalizedDates: [Date, Date] = [from, to];
  const ranges = usageDashboardRanges();
  const matchingPreset = presets.find((preset) => {
    const presetDates = usageDatePickerRange(ranges[preset]);
    return (
      presetDates &&
      sameDate(presetDates[0], normalizedDates[0]) &&
      sameDate(presetDates[1], normalizedDates[1])
    );
  });

  currentPreset.value = matchingPreset ?? "custom";
  customDates.value = { start: from, end: to };
  selectedRange.value = matchingPreset ? ranges[matchingPreset] : customUsageRange(from, to);
  void usage.loadDashboard(selectedRange.value);
}

function openSettings(): void {
  void navigateMain(router, "settings", "settings-title");
}

onMounted(() => {
  void usage.loadDashboard(selectedRange.value);
});

onBeforeUnmount(() => {
  if (refreshPoll) {
    clearInterval(refreshPoll);
    refreshPoll = null;
  }
});
</script>

<template>
  <main class="usage-page" :aria-label="t('a11y.usageRegion')">
    <p class="visually-hidden" aria-live="polite">{{ liveMessage }}</p>

    <div class="usage-page__inner">
      <header class="usage-page__top">
        <div class="usage-page__heading">
          <h1 id="main-usage-title" tabindex="-1">{{ t("main.title") }}</h1>
        </div>
        <div class="usage-page__tools">
          <div class="usage-page__segmented" role="group" :aria-label="t('main.filter')">
            <button
              v-for="preset in presets"
              :key="preset"
              type="button"
              :aria-pressed="currentPreset === preset"
              :data-selected="currentPreset === preset ? 'true' : undefined"
              @click="selectPreset(preset)"
            >
              {{ t(`main.range.${preset}`) }}
            </button>
          </div>
          <span class="usage-page__scan">{{ scanText }}</span>
          <button
            type="button"
            class="usage-page__refresh button button--quiet"
            :disabled="usage.scanning"
            @click="refreshUsage"
          >
            <span class="usage-page__refresh-icon" aria-hidden="true"></span>
            {{ usage.scanning ? t("main.scanningUsage") : t("main.refreshUsage") }}
          </button>
        </div>
      </header>

      <div v-if="currentPreset === 'custom'" class="usage-page__custom-row">
        <VDatePicker
          v-model.range="customDates"
          :first-day-of-week="2"
          :locale="calendarLocale"
          :max-date="todayDate"
          mode="date"
          @update:model-value="handleCustomRange"
        >
          <template #default="{ inputValue, inputEvents }">
            <input
              class="usage-page__date-input"
              type="text"
              size="26"
              :value="formatDateRangeInput(inputValue.start, inputValue.end)"
              :placeholder="t('main.customRange')"
              :aria-label="t('main.customRange')"
              autocomplete="off"
              readonly
              v-on="inputEvents.start"
            />
          </template>
        </VDatePicker>
      </div>

      <section v-if="!allServicesOff" class="usage-page__kpi" role="group">
        <div v-for="card in kpiCards" :key="card.key" class="kpi-card">
          <span class="kpi-card__label">
            <i
              v-if="card.provider"
              class="kpi-card__mark"
              :data-provider="card.provider"
              aria-hidden="true"
            ></i>
            {{ card.label }}
          </span>
          <span class="kpi-card__value numeric">
            {{ card.text }}<small v-if="card.unit">{{ tokenUnitSeparator }}{{ card.unit }}</small>
            <small
              v-if="card.delta !== null && card.delta !== undefined"
              class="kpi-card__delta"
              :data-up="card.delta >= 0 ? 'true' : undefined"
              aria-hidden="true"
              >{{ card.delta > 0 ? "↑" : "↓" }} {{ Math.abs(card.delta).toFixed(1) }}%</small
            >
          </span>
        </div>
      </section>

      <section
        v-if="allServicesOff"
        class="usage-page__empty"
        aria-labelledby="usage-empty-heading"
      >
        <h2 id="usage-empty-heading" class="visually-hidden">{{ t("main.noServices") }}</h2>
        <p>{{ t("main.allServicesOff") }}</p>
        <p class="usage-page__empty-hint">{{ t("main.allServicesOffHint") }}</p>
        <button type="button" class="button button--quiet" @click="openSettings">
          {{ t("main.openSettings") }}
        </button>
      </section>

      <section
        v-if="!allServicesOff"
        class="usage-page__block"
        aria-labelledby="usage-breakdown-heading"
      >
        <div class="usage-page__block-head">
          <h2 id="usage-breakdown-heading">{{ t("main.tokenBreakdownPanel") }}</h2>
        </div>
        <div class="usage-page__breakdown">
          <div class="breakdown-hero">
            <span class="breakdown-hero__label">{{ t("main.totalTokens") }}</span>
            <span class="breakdown-hero__value numeric">{{
              breakdown.ready ? breakdown.totalText : t("main.noValue")
            }}</span>
          </div>
          <div class="breakdown-bar" role="img" :aria-label="t('main.tokenBreakdownPanel')">
            <i
              v-for="segment in breakdown.segments"
              :key="segment.opacity"
              :style="{
                inlineSize:
                  breakdown.total > 0 ? `${(segment.tokens / breakdown.total) * 100}%` : '0%',
                opacity: segment.opacity,
              }"
            ></i>
          </div>
          <dl class="breakdown-stats">
            <div>
              <dt>{{ t("main.input") }}</dt>
              <dd class="numeric">{{ breakdown.input }}</dd>
            </div>
            <div>
              <dt>{{ t("main.output") }}</dt>
              <dd class="numeric">{{ breakdown.output }}</dd>
            </div>
            <div>
              <dt>{{ t("main.cacheHit") }}</dt>
              <dd class="numeric">{{ breakdown.cacheRead }}</dd>
            </div>
            <div>
              <dt>{{ t("main.cacheHitRate") }}</dt>
              <dd class="numeric">{{ breakdown.hitRate ?? t("main.noValue") }}</dd>
            </div>
          </dl>
          <dl v-if="breakdown.hasFast" class="breakdown-fast">
            <div>
              <dt>{{ t("main.fastTokens") }}</dt>
              <dd class="numeric">{{ breakdown.fastTotal }}</dd>
            </div>
            <div>
              <dt>{{ t("main.billingEquivalentTokens") }}</dt>
              <dd class="numeric">{{ breakdown.billingEquivalent }}</dd>
            </div>
            <div v-if="breakdown.multiplier">
              <dt>{{ t("main.fastMultiplier") }}</dt>
              <dd class="numeric">{{ breakdown.multiplier }}</dd>
            </div>
            <div v-if="breakdown.fastShare !== null">
              <dt>{{ t("main.fastShare") }}</dt>
              <dd class="numeric">{{ breakdown.fastShare }}%</dd>
            </div>
          </dl>
        </div>
      </section>

      <section class="usage-page__block" aria-labelledby="usage-provider-heading">
        <div class="usage-page__block-head">
          <h2 id="usage-provider-heading">{{ t("main.byProvider") }}</h2>
        </div>
        <div
          class="usage-page__providers"
          :class="{ 'usage-page__providers--single': providerSources.length === 1 }"
        >
          <UsageProviderCard
            v-for="source in providerSources"
            :key="source"
            :source="source"
            :summary="sourceSummary"
            :total-tokens="providerTotalTokens"
            :loaded="dashboardReady"
            :unavailable="usage.dashboardUnavailable"
          />
        </div>
      </section>

      <section class="usage-page__block" aria-labelledby="usage-daily-heading">
        <div class="usage-page__block-head usage-page__block-head--with-legend">
          <h2 id="usage-daily-heading">{{ t("main.dailyUsage") }}</h2>
          <div class="usage-page__chart-meta">
            <span v-if="chartUsesContextWindow" class="usage-page__chart-note">
              {{ t("main.chartContext") }}
            </span>
            <div class="usage-page__legend" role="list" :aria-label="t('main.byProvider')">
              <span
                v-for="source in providerSources"
                :key="source"
                class="usage-page__legend-item"
                :data-provider="source"
                role="listitem"
              >
                <span class="usage-page__legend-dot" aria-hidden="true"></span>
                {{ t(`provider.${source}`) }}
              </span>
            </div>
          </div>
        </div>
        <UsageDailyChart
          :day="usage.dashboard.day"
          :sources="providerSources"
          :range="selectedRange"
          :chart-range="chartRange"
          :loaded="dashboardReady"
          :unavailable="usage.dashboardUnavailable"
        />
      </section>

      <section
        class="usage-page__block usage-page__block--last"
        aria-labelledby="usage-model-heading"
      >
        <div class="usage-page__block-head">
          <h2 id="usage-model-heading">{{ t("main.byModel") }}</h2>
        </div>
        <UsageModelTable
          :model="usage.dashboard.model"
          :sources="providerSources"
          :source-summary="sourceSummary"
          :loaded="dashboardReady"
          :unavailable="usage.dashboardUnavailable"
        />
      </section>
    </div>
  </main>
</template>

<style scoped>
.usage-page {
  --usage-canvas: var(--surface-primary);
  --usage-surface: var(--surface-raised);
  --usage-divider: var(--border-subtle);
  --usage-track: var(--track-background);
  /* 断点按内容区而非视口：侧边栏 176px 不参与窄屏判断（ADR-0024 后内容区 = 视口 − 176px） */
  container-type: inline-size;
  min-block-size: 100vh;
  padding: clamp(1.5rem, 3vw, 2rem) clamp(1.5rem, 3vw, 2.5rem) 2.5rem;
  background: var(--usage-canvas);
  font-family: var(--font-ui);
}

.usage-page__inner {
  inline-size: min(100%, 75rem);
  margin-inline: auto;
}

.usage-page__top,
.usage-page__heading,
.usage-page__tools,
.usage-page__custom-row,
.usage-page__block-head {
  display: flex;
  align-items: center;
}

.usage-page__top {
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  flex-wrap: wrap;
  padding-block-end: 0.75rem;
  border-block-end: 1px solid var(--usage-divider);
  margin-block-end: 1.25rem;
}

.usage-page__heading {
  align-items: baseline;
  min-inline-size: 0;
  gap: var(--space-4);
}

.usage-page__heading h1 {
  margin: 0;
  font-size: 1.5rem;
  font-weight: 700;
  letter-spacing: -0.025em;
  line-height: 1.15;
}

/* 辅助聚焦目标：程序化 focus 不画 outline，键盘 Tab 的控件仍走全局 :focus-visible */
.usage-page__heading h1[tabindex="-1"]:focus {
  outline: none;
}

.usage-page__tools {
  gap: 0.625rem;
  min-inline-size: 0;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.usage-page__scan {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
  white-space: nowrap;
}

.usage-page__scan::before {
  inline-size: 0.3125rem;
  block-size: 0.3125rem;
  flex: 0 0 auto;
  border-radius: 50%;
  background: var(--text-secondary);
  content: "";
  opacity: 0.5;
}

.usage-page__refresh {
  min-block-size: 2.25rem;
  padding-inline: 0.625rem;
  font-size: 0.75rem;
}

.usage-page__refresh-icon {
  inline-size: 0.75rem;
  block-size: 0.75rem;
  border: 1.5px solid currentColor;
  border-block-start-color: transparent;
  border-radius: 50%;
}

@media (prefers-reduced-motion: no-preference) {
  .usage-page__refresh:not(:disabled) .usage-page__refresh-icon {
    transition: transform var(--motion-base) var(--ease-out);
  }

  .usage-page__refresh:not(:disabled):hover .usage-page__refresh-icon {
    transform: rotate(180deg);
  }
}

.usage-page__custom-row {
  gap: 0.625rem;
  margin-block-end: 1.25rem;
}

.usage-page__empty {
  padding: 2.5rem 1rem;
  border: 1px dashed var(--border-subtle);
  border-radius: 0.625rem;
  margin-block-end: 1.25rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

.usage-page__empty p {
  margin: 0 0 0.5rem;
}

.usage-page__empty-hint {
  font-size: 0.6875rem;
}

/* KPI 总览行：数字是整页唯一的大字号，标签退到次文字层级（贴合式层级方向） */
.usage-page__kpi {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(10.5rem, 1fr));
  gap: 0.75rem;
  margin-block-end: 1.25rem;
}

.kpi-card {
  display: grid;
  gap: 0.3125rem;
  min-inline-size: 0;
  padding: 1rem 1.125rem;
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
}

.kpi-card__label {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  min-inline-size: 0;
  overflow: hidden;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.kpi-card__mark {
  inline-size: 0.4375rem;
  block-size: 0.4375rem;
  flex: 0 0 auto;
  border-radius: 0.125rem;
  background: var(--cat-codex);
}

.kpi-card__mark[data-provider="claude"] {
  background: var(--cat-claude);
}

.kpi-card__mark[data-provider="pi"] {
  background: var(--cat-pi);
}

.kpi-card__mark[data-provider="opencode"] {
  background: var(--cat-opencode);
}

.kpi-card__value {
  min-inline-size: 0;
  overflow: hidden;
  color: var(--text-primary);
  font-size: 1.375rem;
  font-weight: 600;
  letter-spacing: -0.02em;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.kpi-card__value small {
  margin-inline-start: 0.125rem;
  font-size: 0.6em;
  font-weight: 550;
}

.kpi-card__value small.kpi-card__delta {
  margin-inline-start: 0.375rem;
  font-size: 0.6875rem;
  font-weight: 600;
  color: var(--status-success);
}

.kpi-card__value small.kpi-card__delta[data-up="true"] {
  color: var(--status-error);
}

/* Token 拆分面板：hero 大数字 + 堆叠条 + 分项 + Fast 汇总 */
.usage-page__breakdown {
  display: grid;
  gap: 0.75rem;
  padding: 1rem 1.125rem;
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
}

.breakdown-hero {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 1rem;
}

.breakdown-hero__label {
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.breakdown-hero__value {
  color: var(--text-primary);
  font-size: 1.375rem;
  font-weight: 600;
  letter-spacing: -0.02em;
  line-height: 1.2;
}

.breakdown-bar {
  display: flex;
  block-size: 8px;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in srgb, var(--text-primary) 18%, transparent);
}

.breakdown-bar > i {
  display: block;
  block-size: 100%;
  background: var(--text-primary);
}

.breakdown-stats,
.breakdown-fast {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(8rem, 1fr));
  gap: 0.625rem 1.25rem;
  margin: 0;
}

.breakdown-stats div,
.breakdown-fast div {
  min-inline-size: 0;
}

.breakdown-stats dt,
.breakdown-fast dt {
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.breakdown-stats dd,
.breakdown-fast dd {
  margin: 0.125rem 0 0;
  font-size: 0.8125rem;
  font-weight: 600;
}

.breakdown-fast {
  padding-block-start: 0.75rem;
  border-block-start: 1px solid var(--usage-divider);
}

.breakdown-fast dt {
  color: var(--text-tertiary);
}

.usage-page__segmented {
  display: inline-flex;
  max-inline-size: 100%;
  overflow-x: auto;
  padding: 0.1875rem;
  border-radius: var(--radius-control);
  background: var(--usage-track);
  scrollbar-width: none;
}

.usage-page__segmented::-webkit-scrollbar {
  display: none;
}

.usage-page__segmented button {
  min-block-size: 2.5rem;
  padding: 0 0.75rem;
  border: 0;
  border-radius: calc(var(--radius-control) - 0.1875rem);
  color: var(--text-secondary);
  background: transparent;
  font-size: 0.78125rem;
  white-space: nowrap;
}

.usage-page__segmented button:hover {
  color: var(--text-primary);
}

.usage-page__segmented button[data-selected="true"] {
  color: var(--text-primary);
  background: var(--usage-surface);
  box-shadow: 0 1px 2px rgb(24 24 27 / 10%);
  font-weight: 570;
}

/* 日期范围框：产物 date-input（40px 高、1px 边框、9px 圆角、surface-raised 底） */
.usage-page__date-input {
  min-block-size: 2.5rem;
  padding: 0 0.875rem;
  color: var(--text-secondary);
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid var(--border-subtle);
  border-radius: 0.5625rem;
  font-size: 0.78125rem;
  font-variant-numeric: tabular-nums;
}

.usage-page__date-input:focus-visible {
  outline: 2px solid var(--action-primary);
  outline-offset: 2px;
}

.usage-page__providers {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 1rem;
  align-items: stretch;
}

.usage-page__providers--single {
  grid-template-columns: 1fr;
}

.usage-page__block {
  margin-block-end: 1.25rem;
}

.usage-page__block--last {
  margin-block-end: 0;
}

.usage-page__block-head {
  justify-content: space-between;
  min-block-size: 1.5rem;
  gap: 1.25rem;
  margin-block-end: 0.75rem;
}

.usage-page__block-head h2 {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.8125rem;
  font-weight: 600;
}

.usage-page__legend {
  display: flex;
  align-items: center;
  gap: 0.875rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.usage-page__chart-meta {
  display: flex;
  align-items: center;
  gap: 1rem;
  min-inline-size: 0;
}

.usage-page__chart-note {
  color: var(--text-secondary);
  font-size: 0.6875rem;
  white-space: nowrap;
}

.usage-page__legend-item {
  --provider-color: var(--cat-codex);
  display: inline-flex;
  align-items: center;
  gap: 0.3125rem;
  white-space: nowrap;
}

.usage-page__legend-item[data-provider="claude"] {
  --provider-color: var(--cat-claude);
}

.usage-page__legend-item[data-provider="pi"] {
  --provider-color: var(--cat-pi);
}

.usage-page__legend-item[data-provider="opencode"] {
  --provider-color: var(--cat-opencode);
}

.usage-page__legend-dot {
  inline-size: 0.4375rem;
  block-size: 0.4375rem;
  border-radius: 0.125rem;
  background: var(--provider-color);
}

@container (max-width: 640px) {
  .usage-page__heading {
    align-items: flex-start;
    flex-direction: column;
    gap: var(--space-1);
  }

  .usage-page__top {
    align-items: flex-start;
  }

  .usage-page__chart-meta {
    align-items: flex-end;
    flex-direction: column;
    gap: 0.25rem;
  }

  .usage-page__providers {
    grid-template-columns: 1fr;
  }
}

@media (prefers-reduced-motion: no-preference) {
  .usage-page__segmented button {
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out),
      scale var(--motion-fast) var(--ease-out);
  }

  .usage-page__segmented button:active {
    scale: 0.96;
  }
}

/*
 * 修复 v-calendar 顶部导航按钮透出浏览器 UA 默认按钮底色（浅色 #EFEFEF、
 * 深色 #6B6B6B 的灰块）。组件默认是透明底、hover 才出底色。
 * 日历 popover 由 popper 挂载到 body（teleport），scoped 选择器够不到，用 :global。
 */
:global(.vc-header .vc-arrow),
:global(.vc-header .vc-title) {
  background: transparent;
}
</style>
