<script setup lang="ts">
/**
 * 主窗口本地用量页。
 *
 * 页面只读取 Rust 本地用量摘要，不读取额度，也不把 Conversations、设置等后续能力
 * 提前塞进主窗口。布局基线来自 `prototypes/usage-page/index.html` 与 ADR-0020。
 */
import { DatePicker as VDatePicker } from "v-calendar";
import "v-calendar/style.css";
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";

import UsageDailyChart from "../components/UsageDailyChart.vue";
import UsageModelTable from "../components/UsageModelTable.vue";
import UsageProviderCard from "../components/UsageProviderCard.vue";
import { navigateMain } from "../features/app/navigation";
import type { UsageDashboardRange } from "../features/usage/contracts";
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

const scanText = computed(() => {
  if (usage.scanning) return t("main.loading");
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
</script>

<template>
  <main class="usage-page" :aria-label="t('a11y.usageRegion')">
    <p class="visually-hidden" aria-live="polite">{{ liveMessage }}</p>

    <div class="usage-page__inner">
      <header class="usage-page__top">
        <div class="usage-page__heading">
          <h1 id="main-usage-title" tabindex="-1">{{ t("main.title") }}</h1>
          <span class="usage-page__scan">{{ scanText }}</span>
        </div>
      </header>

      <div class="usage-page__filters" role="group" :aria-label="t('main.filter')">
        <div class="usage-page__segmented" role="group">
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

      <section class="usage-page__block" aria-labelledby="usage-provider-heading">
        <div class="usage-page__block-head">
          <h2 id="usage-provider-heading">{{ t("main.byProvider") }}</h2>
          <span v-if="!allServicesOff" class="usage-page__summary numeric">
            <span
              >{{ t("main.totalTokens") }}
              <b :title="totalTokens?.full"
                >{{ totalTokens?.value ?? t("main.noValue")
                }}<small v-if="totalTokens?.unit"
                  >{{ tokenUnitSeparator }}{{ totalTokens.unit }}</small
                ></b
              ></span
            >
            <span
              >{{ t("main.totalCost") }}
              <b class="usage-page__summary-money">{{ totalCost ?? t("main.noValue") }}</b></span
            >
          </span>
        </div>
        <div class="usage-page__providers">
          <UsageProviderCard
            v-for="source in providerSources"
            :key="source"
            :source="source"
            :summary="sourceSummary"
            :loaded="dashboardReady"
            :unavailable="usage.dashboardUnavailable"
          />
        </div>
      </section>

      <section class="usage-page__block" aria-labelledby="usage-daily-heading">
        <div class="usage-page__block-head usage-page__block-head--with-legend">
          <h2 id="usage-daily-heading">{{ t("main.dailyCost") }}</h2>
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
  --usage-card-shadow: var(--shadow-lane);
  min-block-size: 100vh;
  padding: clamp(1.125rem, 3vw, 1.375rem) clamp(1.125rem, 3vw, 1.875rem) 2.125rem;
  background: var(--usage-canvas);
  font-family: var(--font-ui);
}

.usage-page__inner {
  inline-size: min(100%, 75rem);
  margin-inline: auto;
}

.usage-page__top,
.usage-page__heading,
.usage-page__filters,
.usage-page__block-head {
  display: flex;
  align-items: center;
}

.usage-page__top {
  align-items: baseline;
  justify-content: space-between;
  gap: var(--space-4);
  padding-block-end: 0.625rem;
  border-block-end: 1px solid var(--usage-divider);
  margin-block-end: 0.875rem;
}

.usage-page__heading {
  align-items: baseline;
  min-inline-size: 0;
  gap: var(--space-4);
}

.usage-page__heading h1 {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 680;
  letter-spacing: -0.025em;
  line-height: 1.15;
}

/* 辅助聚焦目标：程序化 focus 不画 outline，键盘 Tab 的控件仍走全局 :focus-visible */
.usage-page__heading h1[tabindex="-1"]:focus {
  outline: none;
}

.usage-page__scan {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
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

.usage-page__filters {
  flex-wrap: wrap;
  gap: 0.625rem;
  margin-block-end: 1rem;
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
  min-block-size: 1.875rem;
  padding: 0 0.625rem;
  border: 0;
  border-radius: calc(var(--radius-control) - 0.1875rem);
  color: var(--text-secondary);
  background: transparent;
  font-size: 0.71875rem;
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

/* 日期范围框：产物 date-input（32px 高、1px 边框、9px 圆角、surface-raised 底） */
.usage-page__date-input {
  min-block-size: 2rem;
  padding: 0 0.75rem;
  color: var(--text-secondary);
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid var(--border-subtle);
  border-radius: 0.5625rem;
  font-size: 0.71875rem;
  font-variant-numeric: tabular-nums;
}

.usage-page__date-input:focus-visible {
  outline: 2px solid var(--action-primary);
  outline-offset: 2px;
}

/* 摘要挂「按 Provider」标题行右侧（ADR-0024）：钱为主、Token 为次 */
.usage-page__summary {
  order: 1;
  display: inline-flex;
  align-items: baseline;
  gap: 0.875rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
  font-weight: 520;
  white-space: nowrap;
}

.usage-page__summary b {
  margin-inline-start: 0.3125rem;
  color: var(--text-primary);
  font-size: 0.8125rem;
  font-weight: 650;
}

.usage-page__summary b small {
  margin-inline-start: 0.0625rem;
  font-size: 0.6em;
  font-weight: 550;
}

.usage-page__summary-money {
  font-size: 0.84375rem;
  font-weight: 700;
}

.usage-page__providers {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0.75rem;
  align-items: stretch;
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
  gap: 1rem;
  margin-block-end: 0.5rem;
}

.usage-page__block-head:not(.usage-page__block-head--with-legend)::after {
  block-size: 1px;
  flex: 1 1 auto;
  margin-inline-start: 0.25rem;
  background: var(--usage-divider);
  content: "";
}

.usage-page__block-head h2 {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 680;
  letter-spacing: 0.04em;
}

.usage-page__legend {
  display: flex;
  align-items: center;
  gap: 0.875rem;
  color: var(--text-secondary);
  font-size: 0.65625rem;
}

.usage-page__chart-meta {
  display: flex;
  align-items: center;
  gap: 1rem;
  min-inline-size: 0;
}

.usage-page__chart-note {
  color: var(--text-secondary);
  font-size: 0.65625rem;
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

.usage-page__providers {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.75rem;
}

@media (max-width: 1100px) {
  .usage-page__providers {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 640px) {
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

  .usage-page__summary {
    display: none;
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
