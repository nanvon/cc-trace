<script setup lang="ts">
/**
 * 单个对话的全生命周期详情。
 *
 * 展示 Provider、项目、起止时间与持续时间、请求数、Token 与费用，以及模型／速度拆分。
 * 详情固定展示该对话全量聚合，不受列表筛选影响（对应 cc-bar F-17）。
 */
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";

import { getConversation, getConversationBreakdown } from "../features/usage/api";
import type {
  UsageConversation,
  UsageConversationBreakdown,
  UsageSummaryRow,
  UsageTokenTotals,
} from "../features/usage/contracts";
import {
  formatUsageCost,
  presentUsageTokens,
  usageCacheWriteTokens,
} from "../features/usage/presentation";

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

const { t, locale } = useI18n();
const route = useRoute();
const router = useRouter();

const loading = ref(true);
const unavailable = ref(false);
const missing = ref(false);
const conversation = ref<UsageConversation | null>(null);
const breakdown = ref<UsageConversationBreakdown | null>(null);

const tokens = computed<UsageTokenTotals | null>(() => conversation.value?.tokens ?? null);

const durationText = computed(() => {
  const value = conversation.value;
  if (!value) return null;
  const first = new Date(value.firstAt).getTime();
  const last = new Date(value.lastAt).getTime();
  const minutes = Math.max(0, Math.round((last - first) / 60_000));
  if (minutes < 1) return t("conversations.durationUnderMinute");
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return t("conversations.durationMinutes", { count: minutes });
  if (rest === 0) return t("conversations.durationHours", { count: hours });
  return t("conversations.durationHoursMinutes", { hours, minutes: rest });
});

function formatDateTime(value: string): string {
  return new Intl.DateTimeFormat(locale.value, {
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    month: "numeric",
    year: "numeric",
  }).format(new Date(value));
}

function tokenText(value: number): string {
  const display = presentUsageTokens(locale.value, value);
  return `${display.value}${display.unit}`;
}

function costText(): string {
  const value = conversation.value;
  if (!value) return t("main.noValue");
  return (
    formatUsageCost(locale.value, value.cost, value.entryCount, t("main.lessThanCent")) ??
    t("main.unpriced")
  );
}

function rowCost(row: UsageSummaryRow): string {
  return (
    formatUsageCost(locale.value, row.cost, row.entryCount, t("main.lessThanCent")) ??
    t("main.unpriced")
  );
}

function speedLabel(row: UsageSummaryRow): string {
  if (row.key === "standard") return t("conversations.speedStandard");
  if (row.key === "fast") return t("conversations.speedFast");
  return t("conversations.speedUnknown");
}

function backToList(): void {
  void router.push({ name: "conversations" });
}

function entryCount(): number {
  return conversation.value?.entryCount ?? 0;
}

onMounted(async () => {
  const key = String(route.params.key ?? "");
  if (!key) {
    missing.value = true;
    loading.value = false;
    return;
  }
  try {
    const [detail, parts] = await Promise.all([
      getConversation(key),
      getConversationBreakdown(key),
    ]);
    if (!detail) {
      missing.value = true;
    } else {
      conversation.value = detail;
      breakdown.value = parts;
    }
  } catch {
    unavailable.value = true;
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <main class="conversation-detail" :aria-label="t('a11y.conversationDetailRegion')">
    <div class="conversation-detail__inner">
      <header class="conversation-detail__header">
        <button type="button" class="button button--quiet" @click="backToList">
          <span aria-hidden="true">←</span>
          {{ t("conversations.backToList") }}
        </button>
        <h1 id="conversation-detail-title" tabindex="-1">
          {{ conversation?.title ?? t("conversations.title") }}
        </h1>
      </header>

      <p v-if="unavailable" class="conversation-detail__notice">
        {{ t("conversations.unavailable") }}
      </p>
      <p v-else-if="missing" class="conversation-detail__notice">
        {{ t("conversations.missing") }}
      </p>
      <p v-else-if="loading" class="conversation-detail__notice">
        {{ t("conversations.loading") }}
      </p>

      <template v-else-if="conversation">
        <section class="conversation-detail__block" aria-labelledby="lifecycle-heading">
          <h2 id="lifecycle-heading" class="visually-hidden">{{ t("conversations.lifecycle") }}</h2>
          <dl class="conversation-detail__kpis">
            <div>
              <dt>{{ t("conversations.provider") }}</dt>
              <dd class="conversation-detail__provider">
                <span
                  class="conversation-detail__dot"
                  :data-provider="conversation.source"
                  aria-hidden="true"
                ></span>
                {{ t(`provider.${conversation.source}`) }}
              </dd>
            </div>
            <div v-if="conversation.projectHint">
              <dt>{{ t("conversations.project") }}</dt>
              <dd>{{ conversation.projectHint }}</dd>
            </div>
            <div v-if="conversation.isSidechain">
              <dt>{{ t("conversations.role") }}</dt>
              <dd>{{ t("conversations.sidechain") }}</dd>
            </div>
            <div>
              <dt>{{ t("conversations.firstAt") }}</dt>
              <dd class="numeric">{{ formatDateTime(conversation.firstAt) }}</dd>
            </div>
            <div>
              <dt>{{ t("conversations.lastAt") }}</dt>
              <dd class="numeric">{{ formatDateTime(conversation.lastAt) }}</dd>
            </div>
            <div>
              <dt>{{ t("conversations.duration") }}</dt>
              <dd>{{ durationText }}</dd>
            </div>
            <div>
              <dt>{{ t("conversations.requests") }}</dt>
              <dd class="numeric">{{ entryCount() }}</dd>
            </div>
            <div>
              <dt>{{ t("conversations.totalTokens") }}</dt>
              <dd class="numeric">{{ tokenText(conversation.tokens.totalTokens) }}</dd>
            </div>
            <div>
              <dt>{{ t("conversations.totalCost") }}</dt>
              <dd class="numeric">{{ costText() }}</dd>
            </div>
          </dl>
        </section>

        <section class="conversation-detail__block" aria-labelledby="tokens-heading">
          <h2 id="tokens-heading">{{ t("conversations.tokenBreakdown") }}</h2>
          <dl class="conversation-detail__tokens">
            <div>
              <dt>{{ t("main.input") }}</dt>
              <dd class="numeric">{{ tokenText(tokens?.inputTokens ?? 0) }}</dd>
            </div>
            <div>
              <dt>{{ t("main.output") }}</dt>
              <dd class="numeric">{{ tokenText(tokens?.outputTokens ?? 0) }}</dd>
            </div>
            <div>
              <dt>{{ t("conversations.reasoning") }}</dt>
              <dd class="numeric">{{ tokenText(tokens?.reasoningOutputTokens ?? 0) }}</dd>
            </div>
            <div>
              <dt>{{ t("main.cacheHit") }}</dt>
              <dd class="numeric">{{ tokenText(tokens?.cacheReadInputTokens ?? 0) }}</dd>
            </div>
            <div>
              <dt>{{ t("main.cacheWrite") }}</dt>
              <dd class="numeric">
                {{ tokenText(usageCacheWriteTokens(tokens ?? EMPTY_TOKENS)) }}
              </dd>
            </div>
          </dl>
        </section>

        <section class="conversation-detail__block" aria-labelledby="speed-heading">
          <h2 id="speed-heading">{{ t("conversations.speedTiers") }}</h2>
          <div class="conversation-detail__table-wrap">
            <table class="conversation-detail__table">
              <thead>
                <tr>
                  <th scope="col">{{ t("conversations.tier") }}</th>
                  <th scope="col">{{ t("conversations.rawTokens") }}</th>
                  <th scope="col">{{ t("conversations.billingEquivalent") }}</th>
                  <th scope="col">{{ t("main.cost") }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in breakdown?.speeds ?? []" :key="row.key">
                  <td>{{ speedLabel(row) }}</td>
                  <td class="numeric">{{ tokenText(row.fast.rawTokens) }}</td>
                  <td class="numeric">{{ row.fast.billingEquivalentTokens }}</td>
                  <td class="numeric">{{ rowCost(row) }}</td>
                </tr>
                <tr v-if="(breakdown?.speeds ?? []).length === 0">
                  <td colspan="4">{{ t("main.empty") }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>

        <section class="conversation-detail__block" aria-labelledby="model-heading">
          <h2 id="model-heading">{{ t("conversations.byModel") }}</h2>
          <div class="conversation-detail__table-wrap">
            <table class="conversation-detail__table">
              <thead>
                <tr>
                  <th scope="col">{{ t("main.model") }}</th>
                  <th scope="col">{{ t("conversations.requests") }}</th>
                  <th scope="col">{{ t("main.input") }}</th>
                  <th scope="col">{{ t("main.output") }}</th>
                  <th scope="col">{{ t("main.total") }}</th>
                  <th scope="col">{{ t("main.cost") }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in breakdown?.models ?? []" :key="row.key">
                  <td>{{ row.key || t("main.noValue") }}</td>
                  <td class="numeric">{{ row.entryCount }}</td>
                  <td class="numeric">{{ tokenText(row.tokens.inputTokens) }}</td>
                  <td class="numeric">{{ tokenText(row.tokens.outputTokens) }}</td>
                  <td class="numeric">{{ tokenText(row.tokens.totalTokens) }}</td>
                  <td class="numeric">{{ rowCost(row) }}</td>
                </tr>
                <tr v-if="(breakdown?.models ?? []).length === 0">
                  <td colspan="6">{{ t("main.empty") }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>
      </template>
    </div>
  </main>
</template>

<style scoped>
.conversation-detail {
  --usage-canvas: color-mix(in srgb, var(--surface-primary) 86%, var(--border-subtle) 14%);
  --usage-surface: var(--surface-raised);
  --usage-divider: var(--border-subtle);
  min-block-size: 100vh;
  padding: clamp(1.125rem, 3vw, 1.375rem) clamp(1.125rem, 3vw, 1.875rem) 2.125rem;
  background: var(--usage-canvas);
  font-family: var(--font-ui);
}

.conversation-detail__inner {
  inline-size: min(100%, 75rem);
  margin-inline: auto;
}

.conversation-detail__header {
  display: flex;
  align-items: baseline;
  gap: var(--space-4);
  padding-block-end: 0.625rem;
  border-block-end: 1px solid var(--usage-divider);
  margin-block-end: 1rem;
}

.conversation-detail__header h1 {
  margin: 0;
  overflow: hidden;
  font-size: 1.25rem;
  font-weight: 680;
  letter-spacing: -0.025em;
  line-height: 1.15;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conversation-detail__notice {
  padding: 2.5rem 1rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

.conversation-detail__block {
  margin-block-end: 1.25rem;
  padding: 0.875rem 1rem;
  background: var(--usage-surface);
  border: 1px solid var(--border-subtle);
  border-radius: 0.625rem;
}

.conversation-detail__block h2 {
  margin: 0 0 0.625rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 650;
}

.conversation-detail__kpis {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(11rem, 1fr));
  gap: 0.625rem 1.25rem;
  margin: 0;
}

.conversation-detail__kpis div {
  min-inline-size: 0;
}

.conversation-detail__kpis dt {
  color: var(--text-secondary);
  font-size: 0.59375rem;
}

.conversation-detail__kpis dd {
  margin: 0.125rem 0 0;
  overflow: hidden;
  font-size: 0.75rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conversation-detail__provider {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
}

.conversation-detail__dot {
  inline-size: 0.4375rem;
  block-size: 0.4375rem;
  border-radius: 0.125rem;
  background: var(--cat-codex);
}

.conversation-detail__dot[data-provider="claude"] {
  background: var(--cat-claude);
}

.conversation-detail__tokens {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(9rem, 1fr));
  gap: 0.625rem 1.25rem;
  margin: 0;
}

.conversation-detail__tokens div {
  min-inline-size: 0;
}

.conversation-detail__tokens dt {
  color: var(--text-secondary);
  font-size: 0.59375rem;
}

.conversation-detail__tokens dd {
  margin: 0.125rem 0 0;
  font-size: 0.75rem;
  font-weight: 600;
}

.conversation-detail__table-wrap {
  overflow-x: auto;
  border: 1px solid var(--border-subtle);
  border-radius: 0.5rem;
}

.conversation-detail__table {
  inline-size: 100%;
  min-inline-size: 30rem;
  border-collapse: collapse;
}

.conversation-detail__table th,
.conversation-detail__table td {
  block-size: 2rem;
  padding: 0 0.625rem;
  border-block-end: 1px solid var(--border-subtle);
  font-size: 0.65625rem;
  font-variant-numeric: tabular-nums;
  text-align: right;
  white-space: nowrap;
}

.conversation-detail__table thead th {
  color: var(--text-secondary);
  font-size: 0.59375rem;
  font-weight: 550;
  text-align: left;
}

.conversation-detail__table th:first-child,
.conversation-detail__table td:first-child {
  text-align: left;
}

.conversation-detail__table td {
  font-weight: 500;
}
</style>
