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
  presentUsageTokens,
  usageCacheWriteTokens,
} from "../features/usage/presentation";

type SortKey = "model" | "input" | "output" | "cacheHit" | "cacheWrite" | "total" | "cost";
type SortDirection = "ascending" | "descending";

const props = defineProps<{
  model: Record<UsageSource, UsageSummary | null>;
  sources: readonly UsageSource[];
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
  { key: "total", label: "total" },
  { key: "cost", label: "cost" },
];

const groups = computed(() =>
  props.sources.map((source) => ({
    source,
    summary: props.model[source],
    rows: sortRows(props.model[source]?.rows ?? []),
  })),
);

/* 无任何模型数据（含加载失败/不可用）时压缩整表行高，避免空态占一大块 */
const compact = computed(() => groups.value.every((group) => group.rows.length === 0));

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
      <table class="usage-table" :class="{ 'usage-table--compact': compact }">
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
            <th scope="row" class="usage-table__model">
              <span class="usage-table__m-provider" aria-hidden="true"></span>
              {{ row.key || t("main.noValue") }}
            </th>
            <td>{{ tokenText(row.tokens.inputTokens) }}</td>
            <td>{{ tokenText(row.tokens.outputTokens) }}</td>
            <td>{{ tokenText(row.tokens.cacheReadInputTokens) }}</td>
            <td>{{ tokenText(usageCacheWriteTokens(row.tokens)) }}</td>
            <td>{{ tokenText(row.tokens.totalTokens) }}</td>
            <td :class="{ 'usage-table__unpriced': row.cost.pricedEntries === 0 }">
              {{ costText(row) }}
            </td>
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
  padding: 0.375rem 0.375rem 0.25rem;
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
  background: var(--usage-surface, var(--surface-raised));
}

.usage-table-scroll {
  overflow-x: auto;
  background: var(--usage-surface, var(--surface-raised));
}

.usage-table {
  inline-size: 100%;
  min-inline-size: 48rem;
  border-collapse: collapse;
  background: var(--usage-surface, var(--surface-raised));
}

.usage-table th,
.usage-table td {
  padding: 0.625rem 0.75rem;
  border-block-end: 1px solid color-mix(in srgb, var(--border-subtle) 55%, transparent);
  font-size: 0.8125rem;
  font-variant-numeric: tabular-nums;
  text-align: right;
  white-space: nowrap;
}

.usage-table thead th {
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 600;
  letter-spacing: 0.02em;
  border-block-end-color: var(--border-subtle);
}

.usage-table thead th:first-child,
.usage-table tbody th,
.usage-table tfoot th {
  text-align: left;
}

.usage-table tbody td:first-child,
.usage-table tfoot td:first-child {
  font-weight: 600;
}

.usage-table__sort {
  min-block-size: 2.25rem;
  margin: 0 -0.3125rem;
  padding: 0 0.3125rem;
  border: 0;
  color: inherit;
  background: transparent;
  font-size: inherit;
  font-weight: inherit;
  letter-spacing: inherit;
  white-space: nowrap;
}

.usage-table__sort--active {
  color: var(--text-primary);
}

/* 分组行：不用 track 整行底色（深色下突兀），改为字重＋色点＋组上边界清晰线表达小计。
   底部让位给下一条 55% 细线，避免与上一组末行的细线叠成双线。 */
.usage-table__group th,
.usage-table__group td {
  background: transparent;
  color: var(--text-primary);
  font-size: 0.6875rem;
  font-weight: 680;
  border-block-end: 0;
  border-block-start: 1px solid var(--border-subtle);
}

.usage-table__group th {
  padding-inline-start: 0.75rem;
}

.usage-table__dot {
  display: inline-block;
  inline-size: 0.4375rem;
  block-size: 0.4375rem;
  margin-inline-end: 0.4375rem;
  border-radius: 0.15625rem;
  background: var(--cat-codex);
  vertical-align: 0.0625rem;
}

tbody[data-provider="claude"] .usage-table__dot {
  background: var(--cat-claude);
}

tbody[data-provider="pi"] .usage-table__dot {
  background: var(--cat-pi);
}

tbody[data-provider="opencode"] .usage-table__dot {
  background: var(--cat-opencode);
}

/* 模型行前的 Provider 色点（产物 m-provider）：色点标识归属，不占语义色 */
.usage-table__m-provider {
  display: inline-block;
  inline-size: 0.5rem;
  block-size: 0.5rem;
  margin-inline-end: 0.4375rem;
  border-radius: 0.1875rem;
  background: var(--cat-codex);
  vertical-align: 0.0625rem;
}

tbody[data-provider="claude"] .usage-table__m-provider {
  background: var(--cat-claude);
}

tbody[data-provider="pi"] .usage-table__m-provider {
  background: var(--cat-pi);
}

tbody[data-provider="opencode"] .usage-table__m-provider {
  background: var(--cat-opencode);
}

.usage-table__model {
  padding-inline-start: 1.4375rem !important;
  font-family: var(--font-data);
  font-size: 0.8125rem !important;
  font-weight: 500;
}

.usage-table__unpriced {
  color: var(--text-secondary);
}

.usage-table tfoot th,
.usage-table tfoot td {
  block-size: 2.1875rem;
  border-block-end: 0;
  border-block-start: 1px solid color-mix(in srgb, var(--border-subtle) 55%, transparent);
  color: var(--text-primary);
  font-weight: 650;
}

/* 空态压缩：无任何模型数据时收紧行高（表头 47 → ~35、分组行 32 → ~26、合计 35 → 28） */
.usage-table--compact th,
.usage-table--compact td {
  padding-block: 0.375rem;
}

.usage-table--compact .usage-table__sort {
  min-block-size: 1.375rem;
}

.usage-table--compact tfoot th,
.usage-table--compact tfoot td {
  block-size: 1.75rem;
}
</style>
