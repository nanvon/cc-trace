<script setup lang="ts">
/**
 * 单个对话的全生命周期详情面板（对话页右侧分栏）。
 *
 * 展示 Provider、项目、起止时间与持续时间、请求数、Token 与费用，以及模型／速度拆分。
 * 详情固定展示该对话全量聚合，不受列表筛选影响（对应 cc-bar F-17）。
 */
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";

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

const props = defineProps<{ conversationKey: string | null }>();

const { t, locale } = useI18n();

const loading = ref(false);
const unavailable = ref(false);
const missing = ref(false);
const conversation = ref<UsageConversation | null>(null);
const breakdown = ref<UsageConversationBreakdown | null>(null);
const copied = ref(false);
let request = 0;

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

/** Fast 倍率：单值或最小–最大范围。 */
function multiplierText(row: UsageSummaryRow): string | null {
  const fast = row.fast;
  if (!fast.minimumMultiplier) return null;
  if (fast.maximumMultiplier && fast.maximumMultiplier !== fast.minimumMultiplier) {
    return `${fast.minimumMultiplier}–${fast.maximumMultiplier}×`;
  }
  return `${fast.minimumMultiplier}×`;
}

function entryCount(): number {
  return conversation.value?.entryCount ?? 0;
}

function copySourceId(): void {
  const id = conversation.value?.sourceId;
  if (!id) return;
  void navigator.clipboard.writeText(id).then(() => {
    copied.value = true;
    setTimeout(() => {
      copied.value = false;
    }, 1500);
  });
}

watch(
  () => props.conversationKey,
  () => {
    void load();
  },
);

onMounted(() => {
  void load();
});

async function load(): Promise<void> {
  const key = props.conversationKey;
  const current = ++request;
  conversation.value = null;
  breakdown.value = null;
  missing.value = false;
  unavailable.value = false;
  if (!key) {
    loading.value = false;
    return;
  }
  loading.value = true;
  try {
    const detail = await getConversation(key);
    if (current !== request) return;
    if (!detail) {
      missing.value = true;
    } else {
      conversation.value = detail;
      // 拆分表失败只降级为空表，不让整页不可用。
      try {
        breakdown.value = await getConversationBreakdown(key);
      } catch {
        breakdown.value = null;
      }
    }
  } catch {
    if (current === request) unavailable.value = true;
  } finally {
    if (current === request) loading.value = false;
  }
}
</script>

<template>
  <div class="cdetail" :aria-label="t('a11y.conversationDetailRegion')">
    <p v-if="unavailable" class="cdetail__notice">{{ t("conversations.unavailable") }}</p>
    <p v-else-if="missing" class="cdetail__notice">{{ t("conversations.missing") }}</p>
    <p v-else-if="!conversationKey" class="cdetail__notice">
      {{ t("conversations.selectConversationHint") }}
    </p>
    <p v-else-if="loading" class="cdetail__notice">{{ t("conversations.loading") }}</p>

    <template v-else-if="conversation">
      <section class="cdetail__block" aria-labelledby="lifecycle-heading">
        <h2 id="lifecycle-heading" class="visually-hidden">{{ t("conversations.lifecycle") }}</h2>
        <dl class="cdetail__kpis">
          <div>
            <dt>{{ t("conversations.provider") }}</dt>
            <dd class="cdetail__provider">
              <span
                class="cdetail__dot"
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
          <div v-if="conversation.branch">
            <dt>{{ t("conversations.branch") }}</dt>
            <dd>{{ conversation.branch }}</dd>
          </div>
          <div v-if="conversation.sourceId">
            <dt>{{ t("conversations.conversationId") }}</dt>
            <dd class="cdetail__id">
              <span class="numeric">{{ conversation.sourceId }}</span>
              <button type="button" class="cdetail__copy" @click="copySourceId">
                {{ copied ? t("conversations.idCopied") : t("conversations.copyId") }}
              </button>
            </dd>
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

      <section class="cdetail__block" aria-labelledby="tokens-heading">
        <h2 id="tokens-heading">{{ t("conversations.tokenBreakdown") }}</h2>
        <dl class="cdetail__tokens">
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

      <section class="cdetail__block" aria-labelledby="speed-heading">
        <h2 id="speed-heading">{{ t("conversations.speedTiers") }}</h2>
        <ul v-if="(breakdown?.speeds ?? []).length > 0" class="cdetail__rows">
          <li v-for="row in breakdown?.speeds ?? []" :key="row.key" class="cdetail__row">
            <div class="cdetail__row-head">
              <span class="cdetail__row-name">{{ speedLabel(row) }}</span>
              <span class="cdetail__row-nums">
                <span class="numeric">{{ tokenText(row.fast.rawTokens) }}</span>
                <strong class="numeric">{{ rowCost(row) }}</strong>
              </span>
            </div>
            <p class="cdetail__row-sub">
              {{ t("conversations.requestCount", { count: row.entryCount }) }}
              · {{ t("conversations.billingEquivalent") }} {{ row.fast.billingEquivalentTokens }}
              <template v-if="multiplierText(row)"> · {{ multiplierText(row) }}</template>
            </p>
          </li>
        </ul>
        <p v-else class="cdetail__empty">{{ t("main.empty") }}</p>
      </section>

      <section class="cdetail__block" aria-labelledby="model-heading">
        <h2 id="model-heading">{{ t("conversations.byModel") }}</h2>
        <ul v-if="(breakdown?.models ?? []).length > 0" class="cdetail__rows">
          <li v-for="row in breakdown?.models ?? []" :key="row.key" class="cdetail__row">
            <div class="cdetail__row-head">
              <span class="cdetail__row-name cdetail__row-name--model">
                {{ row.key || t("main.noValue") }}
              </span>
              <span class="cdetail__row-nums">
                <span class="numeric">{{ tokenText(row.tokens.totalTokens) }}</span>
                <strong class="numeric">{{ rowCost(row) }}</strong>
              </span>
            </div>
            <p class="cdetail__row-sub">
              {{ t("conversations.requestCount", { count: row.entryCount }) }}
              · {{ t("main.input") }} {{ tokenText(row.tokens.inputTokens) }} ·
              {{ t("main.output") }} {{ tokenText(row.tokens.outputTokens) }} ·
              {{ t("main.cacheHit") }} {{ tokenText(row.tokens.cacheReadInputTokens) }}
            </p>
          </li>
        </ul>
        <p v-else class="cdetail__empty">{{ t("main.empty") }}</p>
      </section>
    </template>
  </div>
</template>

<style scoped>
.cdetail {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  min-block-size: 100%;
  padding: 1.25rem clamp(1.125rem, 3vw, 1.5rem) 2.125rem;
}

.cdetail__notice {
  padding: 2.5rem 1rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

.cdetail__block {
  padding: 0.875rem 1rem;
  background: var(--usage-surface);
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
}

.cdetail__block h2 {
  margin: 0 0 0.625rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 600;
}

.cdetail__kpis {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(11rem, 1fr));
  gap: 0.625rem 1.25rem;
  margin: 0;
}

.cdetail__kpis div {
  min-inline-size: 0;
}

.cdetail__kpis dt {
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.cdetail__kpis dd {
  margin: 0.125rem 0 0;
  overflow: hidden;
  font-size: 0.8125rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cdetail__provider {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
}

.cdetail__dot {
  inline-size: 0.4375rem;
  block-size: 0.4375rem;
  border-radius: 0.125rem;
  background: var(--cat-codex);
}

.cdetail__dot[data-provider="claude"] {
  background: var(--cat-claude);
}

.cdetail__dot[data-provider="pi"] {
  background: var(--cat-pi);
}

.cdetail__dot[data-provider="opencode"] {
  background: var(--cat-opencode);
}

.cdetail__id {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-inline-size: 0;
}

.cdetail__id span {
  overflow: hidden;
  text-overflow: ellipsis;
}

.cdetail__copy {
  flex: 0 0 auto;
  padding: 0;
  border: 0;
  color: var(--action-primary);
  background: transparent;
  font-size: 0.6875rem;
  cursor: pointer;
}

.cdetail__copy:hover {
  text-decoration: underline;
}

.cdetail__tokens {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(9rem, 1fr));
  gap: 0.625rem 1.25rem;
  margin: 0;
}

.cdetail__tokens div {
  min-inline-size: 0;
}

.cdetail__tokens dt {
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.cdetail__tokens dd {
  margin: 0.125rem 0 0;
  font-size: 0.8125rem;
  font-weight: 600;
}

/* 速度档位与按模型：行列表（对齐 cc-bar modelDetails），行间 hairline，无表格壳 */
.cdetail__rows {
  display: grid;
  margin: 0;
  padding: 0;
  list-style: none;
}

.cdetail__row {
  display: grid;
  gap: 0.25rem;
  min-inline-size: 0;
  padding: 0.625rem 0;
}

.cdetail__row + .cdetail__row {
  border-block-start: 1px solid var(--border-hairline);
}

.cdetail__row:last-child {
  padding-block-end: 0;
}

.cdetail__row-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 1rem;
  min-inline-size: 0;
}

.cdetail__row-name {
  overflow: hidden;
  font-size: 0.8125rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cdetail__row-name--model {
  font-family: var(--font-data);
  font-weight: 500;
}

.cdetail__row-nums {
  display: flex;
  align-items: baseline;
  gap: 0.75rem;
  flex: none;
  color: var(--text-secondary);
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
}

.cdetail__row-nums strong {
  color: var(--text-primary);
}

.cdetail__row-sub {
  margin: 0;
  overflow: hidden;
  color: var(--text-secondary);
  font-family: var(--font-data);
  font-size: 0.65625rem;
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cdetail__empty {
  margin: 0;
  padding: 0.625rem 0 0.25rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
}
</style>
