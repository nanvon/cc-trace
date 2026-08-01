<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";

import type {
  UsageSource,
  UsageSummary,
  UsageSummaryRow,
  UsageTokenTotals,
} from "../features/usage/contracts";
import {
  formatUsageCost,
  formatUsagePercent,
  presentUsageTokens,
  usageCacheHitRate,
  usageCacheWriteTokens,
} from "../features/usage/presentation";

type SortKey =
  "model" | "input" | "output" | "cacheHit" | "cacheWrite" | "hitRate" | "total" | "cost";
type SortDirection = "ascending" | "descending";

const props = defineProps<{
  model: Record<UsageSource, UsageSummary | null>;
  sourceSummary: UsageSummary | null;
  loaded: boolean;
  unavailable: boolean;
}>();

const { t, locale } = useI18n();
const sortKey = ref<SortKey>("cost");
const sortDirection = ref<SortDirection>("descending");

const EMPTY_TOKENS: UsageTokenTotals = {
  uncachedInputTokens: 0,
  outputTokens: 0,
  reasoningOutputTokens: 0,
  cacheReadInputTokens: 0,
  cacheWrite5mInputTokens: 0,
  cacheWrite1hInputTokens: 0,
  inputTokens: 0,
  totalTokens: 0,
};

const columns: Array<{ key: SortKey; label: string }> = [
  { key: "model", label: "model" },
  { key: "input", label: "input" },
  { key: "output", label: "output" },
  { key: "cacheHit", label: "cacheHit" },
  { key: "cacheWrite", label: "cacheWrite" },
  { key: "hitRate", label: "cacheHitRate" },
  { key: "total", label: "total" },
  { key: "cost", label: "cost" },
];

const groups = computed(() =>
  (["codex", "claude"] as const).map((source) => ({
    source,
    summary: props.model[source],
    rows: sortRows(props.model[source]?.rows ?? []),
  })),
);

function tokenText(value: number): string {
  if (!props.loaded || props.unavailable) return t("main.noValue");
  const display = presentUsageTokens(locale.value, value);
  const separator = display.unit && !locale.value.toLowerCase().startsWith("zh") ? " " : "";
  return `${display.value}${separator}${display.unit}`;
}

function costText(row: UsageSummaryRow | null): string {
  if (!props.loaded || props.unavailable || !row) return t("main.noValue");
  return (
    formatUsageCost(locale.value, row.cost, row.entryCount, t("main.lessThanCent")) ??
    t("main.unpriced")
  );
}

function rateText(row: UsageSummaryRow | null): string {
  if (!props.loaded || props.unavailable || !row) return t("main.noValue");
  return formatUsagePercent(locale.value, usageCacheHitRate(row.tokens));
}

function sortValue(row: UsageSummaryRow, key: SortKey): number | string {
  switch (key) {
    case "model":
      return row.key;
    case "input":
      return row.tokens.inputTokens;
    case "output":
      return row.tokens.outputTokens;
    case "cacheHit":
      return row.tokens.cacheReadInputTokens;
    case "cacheWrite":
      return usageCacheWriteTokens(row.tokens);
    case "hitRate":
      return usageCacheHitRate(row.tokens) ?? -1;
    case "total":
      return row.tokens.totalTokens;
    case "cost":
      return row.cost.pricedEntries > 0 ? row.cost.apiEquivalentCostNanos : -1;
  }
}

function sortRows(rows: UsageSummaryRow[]): UsageSummaryRow[] {
  return [...rows].sort((left, right) => {
    const leftValue = sortValue(left, sortKey.value);
    const rightValue = sortValue(right, sortKey.value);
    const comparison =
      typeof leftValue === "string" && typeof rightValue === "string"
        ? leftValue.localeCompare(rightValue, locale.value)
        : Number(leftValue) - Number(rightValue);
    return sortDirection.value === "ascending" ? comparison : -comparison;
  });
}

function toggleSort(key: SortKey): void {
  if (sortKey.value === key) {
    sortDirection.value = sortDirection.value === "ascending" ? "descending" : "ascending";
    return;
  }
  sortKey.value = key;
  sortDirection.value = key === "model" ? "ascending" : "descending";
}

function ariaSort(key: SortKey): "ascending" | "descending" | "none" {
  return sortKey.value === key ? sortDirection.value : "none";
}

function summaryTokens(summary: UsageSummary | null): UsageTokenTotals {
  return summary?.tokens ?? EMPTY_TOKENS;
}

function summaryCost(summary: UsageSummary | null): string {
  if (!props.loaded || props.unavailable || !summary) return t("main.noValue");
  return (
    formatUsageCost(locale.value, summary.cost, summary.entryCount, t("main.lessThanCent")) ??
    t("main.unpriced")
  );
}

function providerSummaryValue(summary: UsageSummary | null, key: SortKey): string {
  if (!props.loaded || props.unavailable) {
    return key === "model" ? t("main.subtotal") : t("main.noValue");
  }
  if (!summary || summary.entryCount === 0) {
    return key === "model" ? t("main.subtotal") : t("main.noValue");
  }
  const tokens = summaryTokens(summary);
  switch (key) {
    case "model":
      return t("main.subtotal");
    case "input":
      return tokenText(tokens.inputTokens);
    case "output":
      return tokenText(tokens.outputTokens);
    case "cacheHit":
      return tokenText(tokens.cacheReadInputTokens);
    case "cacheWrite":
      return tokenText(usageCacheWriteTokens(tokens));
    case "hitRate":
      return props.loaded && !props.unavailable
        ? formatUsagePercent(locale.value, usageCacheHitRate(tokens))
        : t("main.noValue");
    case "total":
      return tokenText(tokens.totalTokens);
    case "cost":
      return summaryCost(summary);
  }
}

function totalModelCount(): number {
  return groups.value.reduce((count, group) => count + group.rows.length, 0);
}
</script>

<template>
  <div class="usage-table-wrap">
    <div class="usage-table-scroll">
      <table class="usage-table">
        <caption class="visually-hidden">
          {{
            t("a11y.usageTable")
          }}
        </caption>
        <thead>
          <tr>
            <th
              v-for="column in columns"
              :key="column.key"
              scope="col"
              :aria-sort="ariaSort(column.key)"
            >
              <button
                type="button"
                class="usage-table__sort"
                :class="{ 'usage-table__sort--active': sortKey === column.key }"
                :aria-label="t('a11y.sortColumn', { column: t(`main.${column.label}`) })"
                @click="toggleSort(column.key)"
              >
                {{ t(`main.${column.label}`) }}
                <span v-if="sortKey === column.key" aria-hidden="true">
                  {{ sortDirection === "ascending" ? "↑" : "↓" }}
                </span>
              </button>
            </th>
          </tr>
        </thead>

        <tbody v-for="group in groups" :key="group.source" :data-provider="group.source">
          <tr class="usage-table__group">
            <th scope="row">
              <span class="usage-table__dot" aria-hidden="true"></span>
              {{ t(`provider.${group.source}`) }}
            </th>
            <td v-for="column in columns.slice(1)" :key="column.key">
              {{ providerSummaryValue(group.summary, column.key) }}
            </td>
          </tr>
          <tr v-for="row in group.rows" :key="`${group.source}-${row.key}`">
            <th scope="row" class="usage-table__model">{{ row.key || t("main.noValue") }}</th>
            <td>{{ tokenText(row.tokens.inputTokens) }}</td>
            <td>{{ tokenText(row.tokens.outputTokens) }}</td>
            <td>{{ tokenText(row.tokens.cacheReadInputTokens) }}</td>
            <td>{{ tokenText(usageCacheWriteTokens(row.tokens)) }}</td>
            <td>{{ rateText(row) }}</td>
            <td>{{ tokenText(row.tokens.totalTokens) }}</td>
            <td :class="{ 'usage-table__unpriced': row.cost.pricedEntries === 0 }">
              {{ costText(row) }}
            </td>
          </tr>
          <tr
            v-if="props.loaded && !props.unavailable && group.rows.length === 0"
            class="usage-table__empty"
          >
            <td colspan="8">{{ t("main.empty") }}</td>
          </tr>
        </tbody>

        <tfoot>
          <tr>
            <th scope="row">{{ t("main.totalModels", { count: totalModelCount() }) }}</th>
            <td v-for="column in columns.slice(1)" :key="column.key">
              {{ providerSummaryValue(sourceSummary, column.key) }}
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
  </div>
</template>

<style scoped>
.usage-table-wrap {
  overflow: hidden;
  border: 1px solid var(--border-subtle);
  border-radius: 0.625rem;
  box-shadow: var(--usage-card-shadow, var(--shadow-lane));
}

.usage-table-scroll {
  overflow-x: auto;
  background: var(--usage-surface, var(--surface-raised));
}

.usage-table {
  inline-size: 100%;
  min-inline-size: 54.375rem;
  border-collapse: collapse;
  background: var(--usage-surface, var(--surface-raised));
}

.usage-table th,
.usage-table td {
  block-size: 2.0625rem;
  padding: 0 0.625rem;
  border-block-end: 1px solid var(--border-subtle);
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
  text-align: right;
  white-space: nowrap;
}

.usage-table thead th {
  color: var(--text-secondary);
  font-size: 0.59375rem;
  font-weight: 550;
}

.usage-table thead th:first-child,
.usage-table tbody th,
.usage-table tfoot th {
  text-align: left;
}

.usage-table tbody th,
.usage-table tbody td,
.usage-table tfoot th,
.usage-table tfoot td {
  font-size: 0.65625rem;
  font-weight: 500;
}

.usage-table__sort {
  min-block-size: 1.75rem;
  margin: 0 -0.3125rem;
  padding: 0 0.3125rem;
  border: 0;
  color: inherit;
  background: transparent;
  font-size: inherit;
  font-weight: inherit;
  white-space: nowrap;
}

.usage-table__sort--active {
  color: var(--text-primary);
}

.usage-table__group th,
.usage-table__group td {
  background: var(--usage-track, var(--track-background));
  color: var(--text-primary);
  font-size: 0.6875rem;
  font-weight: 680;
}

.usage-table__group th {
  padding-inline-start: 0.75rem;
}

.usage-table__dot {
  display: inline-block;
  inline-size: 0.375rem;
  block-size: 0.375rem;
  margin-inline-end: 0.4375rem;
  border-radius: 50%;
  background: var(--cat-codex);
  vertical-align: 0.0625rem;
}

tbody[data-provider="claude"] .usage-table__dot {
  background: var(--cat-claude);
}

.usage-table__model {
  padding-inline-start: 1.4375rem !important;
  font-family: var(--font-data);
  font-size: 0.625rem !important;
  font-weight: 500;
}

.usage-table__unpriced {
  color: var(--text-secondary);
}

.usage-table__empty td {
  color: var(--text-secondary);
  text-align: left;
}

.usage-table tfoot th,
.usage-table tfoot td {
  block-size: 2.1875rem;
  border-block-end: 0;
  border-block-start: 1px solid var(--border-subtle);
  background: var(--usage-total, var(--surface-primary));
  color: var(--text-primary);
  font-weight: 650;
}
</style>
