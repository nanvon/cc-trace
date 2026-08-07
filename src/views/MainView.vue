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
const providerSources = ["codex", "claude"] as const;
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

const sourceSummary = computed(() => usage.dashboard.source);
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
  void navigateMain(router, "settings", "settings-title", "quota");
}

function openTimeline(): void {
  void navigateMain(router, "timeline", "timeline-title");
}

function openConversations(): void {
  void navigateMain(router, "conversations", "conversations-title");
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
        <div class="usage-page__top-actions">
          <button
            id="main-timeline-trigger"
            type="button"
            class="button button--quiet"
            @click="openTimeline"
          >
            {{ t("timeline.title") }}
          </button>
          <button
            id="main-conversations-trigger"
            type="button"
            class="button button--quiet"
            @click="openConversations"
          >
            {{ t("conversations.title") }}
          </button>
          <button
            id="main-settings-trigger"
            type="button"
            class="button button--quiet"
            @click="openSettings"
          >
            {{ t("common.settings") }}
          </button>
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

      <section class="usage-page__block" aria-labelledby="usage-provider-heading">
        <div class="usage-page__block-head">
          <h2 id="usage-provider-heading">{{ t("main.byProvider") }}</h2>
        </div>
        <div class="usage-page__provider-layout">
          <article class="usage-page__total-card" :aria-label="t('main.overview')">
            <div class="usage-page__total-row">
              <strong class="usage-page__total-value numeric" :title="totalTokens?.full">
                {{ totalTokens?.value ?? t("main.noValue") }}
                <small v-if="totalTokens?.unit">
                  {{ tokenUnitSeparator }}{{ totalTokens.unit }}
                </small>
              </strong>
              <span class="usage-page__total-label">{{ t("main.totalTokens") }}</span>
            </div>
            <div class="usage-page__total-row">
              <strong class="usage-page__total-value numeric" :title="totalCost ?? undefined">
                {{ totalCost ?? t("main.noValue") }}
              </strong>
              <span class="usage-page__total-label">{{ t("main.totalCost") }}</span>
            </div>
          </article>

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
              <span class="usage-page__legend-item" data-provider="codex" role="listitem">
                <span class="usage-page__legend-dot" aria-hidden="true"></span>
                {{ t("provider.codex") }}
              </span>
              <span class="usage-page__legend-item" data-provider="claude" role="listitem">
                <span class="usage-page__legend-dot" aria-hidden="true"></span>
                {{ t("provider.claude") }}
              </span>
            </div>
          </div>
        </div>
        <UsageDailyChart
          :day="usage.dashboard.day"
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
  --usage-canvas: color-mix(in srgb, var(--surface-primary) 86%, var(--border-subtle) 14%);
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

.usage-page__top > .button {
  min-inline-size: 3.25rem;
  min-block-size: 2.5rem;
  padding-inline: 0.75rem;
  border-radius: var(--radius-control);
  font-size: 0.75rem;
}

.usage-page__top-actions {
  display: flex;
  align-items: center;
  gap: 0.375rem;
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
  font-size: 0.6875rem;
  white-space: nowrap;
}

.usage-page__segmented button:hover {
  color: var(--text-primary);
}

.usage-page__segmented button[data-selected="true"] {
  color: var(--text-primary);
  background: var(--usage-surface);
  box-shadow: 0 1px 2px rgb(24 24 27 / 10%);
  font-weight: 500;
}

.usage-page__provider-layout {
  display: grid;
  grid-template-columns: minmax(12rem, 0.42fr) minmax(0, 2fr);
  gap: 0.75rem;
  align-items: stretch;
}

.usage-page__total-card {
  display: grid;
  grid-template-rows: repeat(2, minmax(0, 1fr));
  min-inline-size: 0;
  min-block-size: 100%;
  padding: 0.75rem 1rem;
  border: 1px solid color-mix(in srgb, var(--usage-divider) 72%, transparent);
  border-radius: 1rem;
  background: color-mix(in srgb, var(--usage-surface) 94%, var(--usage-canvas));
  box-shadow:
    0 1px 2px rgb(17 17 17 / 5%),
    0 10px 24px -18px rgb(17 17 17 / 26%);
}

.usage-page__total-row {
  display: flex;
  align-items: flex-end;
  justify-content: flex-start;
  gap: 0.625rem;
  min-inline-size: 0;
  min-block-size: 0;
  padding-block: 0.75rem;
}

.usage-page__total-row + .usage-page__total-row {
  padding-block-start: 0.875rem;
  border-block-start: 1px solid color-mix(in srgb, var(--usage-divider) 72%, transparent);
}

.usage-page__total-label {
  flex: 0 0 auto;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 500;
  line-height: 1.2;
  letter-spacing: 0.01em;
  white-space: nowrap;
}

.usage-page__total-value {
  display: inline-flex;
  align-items: baseline;
  gap: 0.0625rem;
  flex: 0 1 auto;
  min-inline-size: 0;
  max-inline-size: 100%;
  overflow: hidden;
  color: var(--text-primary);
  font-size: clamp(1.5rem, 2.5vw, 2rem);
  font-weight: 650;
  letter-spacing: -0.035em;
  line-height: 0.96;
  text-align: start;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-page__total-value small {
  margin-inline-start: 0.0625rem;
  color: var(--text-secondary);
  font-size: 0.48em;
  font-weight: 550;
  letter-spacing: -0.02em;
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
  font-weight: 650;
  letter-spacing: 0.01em;
}

.usage-page__legend {
  display: flex;
  align-items: center;
  gap: 0.875rem;
  color: var(--text-secondary);
  font-size: 0.625rem;
}

.usage-page__chart-meta {
  display: flex;
  align-items: center;
  gap: 1rem;
  min-inline-size: 0;
}

.usage-page__chart-note {
  color: var(--text-secondary);
  font-size: 0.625rem;
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

@media (max-width: 900px) {
  .usage-page__provider-layout {
    grid-template-columns: 1fr;
  }

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
</style>
