<script setup lang="ts">
/**
 * Provider 矮宽卡：色点＋名称 / 费用＋总 Token / Token 分类（输入、输出、缓存读、缓存写）／缓存命中率条。
 *
 * Token 分类对齐 cc-bar By service 卡片；Fast 等效 Token 与倍率不上卡。
 * 命中率条复用余量条形态（4px），染 Provider 品牌色降饱和版；命中率数值用 success 绿（「绿=好事」，与 cc-bar 一致）。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { UsageSource, UsageSummary, UsageTokenTotals } from "../features/usage/contracts";
import {
  formatUsageCost,
  formatUsagePercent,
  presentUsageTokens,
  usageCacheHitRate,
  usageCacheWriteTokens,
} from "../features/usage/presentation";

const props = defineProps<{
  source: UsageSource;
  summary: UsageSummary | null;
  loaded: boolean;
  unavailable: boolean;
}>();

const { t, locale } = useI18n();

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

const row = computed(() => props.summary?.rows.find((candidate) => candidate.key === props.source));
const tokens = computed(() => row.value?.tokens ?? EMPTY_TOKENS);
const titleId = computed(() => `usage-provider-${props.source}-title`);
const tokenUnitSeparator = computed(() => (locale.value.toLowerCase().startsWith("zh") ? "" : " "));
const hasData = computed(
  () => props.loaded && !props.unavailable && (row.value?.entryCount ?? 0) > 0,
);
const total = computed(() => presentUsageTokens(locale.value, tokens.value.totalTokens));
const cost = computed(() => {
  if (!hasData.value) return null;
  return formatUsageCost(
    locale.value,
    row.value?.cost ?? null,
    row.value?.entryCount ?? 0,
    t("main.lessThanCent"),
  );
});
const hitRate = computed(() => usageCacheHitRate(tokens.value));

const detail = computed(() => {
  const format = (n: number) => {
    if (!hasData.value) return t("main.noValue");
    const display = presentUsageTokens(locale.value, n);
    return display.unit ? `${display.value}${display.unit}` : display.value;
  };
  const raw = tokens.value;
  return {
    input: format(raw.inputTokens),
    output: format(raw.outputTokens),
    cacheRead: format(raw.cacheReadInputTokens),
    cacheWrite: format(usageCacheWriteTokens(raw)),
  };
});

const providerName = computed(() => t(`provider.${props.source}`));
</script>

<template>
  <article class="pcard" :data-p="source" :aria-labelledby="titleId">
    <div class="pcard-head">
      <i aria-hidden="true"></i>
      <h3 :id="titleId">{{ providerName }}</h3>
      <div class="pcard-meta">
        <strong class="pcard-cost numeric">{{ cost ?? t("main.noValue") }}</strong>
        <span class="pcard-total numeric" :title="total.full">
          <b>{{ hasData ? total.value : t("main.noValue") }}</b
          ><small v-if="hasData && total.unit">{{ tokenUnitSeparator }}{{ total.unit }}</small>
          <span class="pcard-total__unit">{{ t("main.tokenUnit") }}</span>
        </span>
      </div>
    </div>

    <div class="pcard-detail">
      <div class="pcard-detail__row">
        <span class="pcard-detail__item">
          <span class="pcard-detail__label">{{ t("main.input") }}</span>
          <b class="pcard-detail__value numeric">{{ detail.input }}</b>
        </span>
        <span class="pcard-detail__item">
          <span class="pcard-detail__label">{{ t("main.output") }}</span>
          <b class="pcard-detail__value numeric">{{ detail.output }}</b>
        </span>
      </div>
      <div class="pcard-detail__row">
        <span class="pcard-detail__item">
          <span class="pcard-detail__label">{{ t("main.cacheHit") }}</span>
          <b class="pcard-detail__value numeric">{{ detail.cacheRead }}</b>
        </span>
        <span class="pcard-detail__item">
          <span class="pcard-detail__label">{{ t("main.cacheWrite") }}</span>
          <b class="pcard-detail__value numeric">{{ detail.cacheWrite }}</b>
        </span>
      </div>
    </div>

    <div class="pcard-hit">
      <div class="pcard-hit-label">
        <span>{{ t("main.cacheHitRate") }}</span>
        <b class="numeric">{{
          hasData ? formatUsagePercent(locale, hitRate) : t("main.noValue")
        }}</b>
      </div>
      <div class="bar" aria-hidden="true">
        <i :style="{ inlineSize: `${hasData ? (hitRate ?? 0) : 0}%` }"></i>
      </div>
    </div>
  </article>
</template>

<style scoped>
.pcard {
  --provider-color: var(--cat-codex);
  display: grid;
  gap: 0.75rem;
  align-content: start;
  min-inline-size: 0;
  padding: 1rem 1.125rem;
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
}

.pcard[data-p="claude"] {
  --provider-color: var(--cat-claude);
}

.pcard[data-p="pi"] {
  --provider-color: var(--cat-pi);
}

.pcard[data-p="opencode"] {
  --provider-color: var(--cat-opencode);
}

.pcard-head {
  display: flex;
  align-items: center;
  gap: 0.4375rem;
  min-inline-size: 0;
}

.pcard-head i {
  inline-size: 0.5rem;
  block-size: 0.5rem;
  flex: 0 0 auto;
  border-radius: 0.1875rem;
  background: var(--provider-color);
}

.pcard-head h3 {
  margin: 0;
  min-inline-size: 0;
  overflow: hidden;
  font-size: 0.875rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pcard-meta {
  display: flex;
  align-items: baseline;
  gap: 0.5rem;
  margin-inline-start: auto;
  min-inline-size: 0;
}

.pcard-cost {
  flex: 0 0 auto;
  color: var(--text-primary);
  font-size: 0.8125rem;
  font-weight: 600;
  letter-spacing: -0.01em;
}

.pcard-total {
  min-inline-size: 0;
  overflow: hidden;
  color: var(--text-secondary);
  font-size: 0.8125rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pcard-total b {
  color: var(--text-primary);
  font-size: 0.8125rem;
  font-weight: 600;
  letter-spacing: -0.01em;
}

.pcard-total small {
  margin-inline-start: 0.125rem;
}

.pcard-total__unit {
  margin-inline-start: 0.25rem;
}

.pcard-detail {
  display: grid;
  gap: 0.375rem;
}

.pcard-detail__row {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0.875rem;
}

.pcard-detail__item {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.5rem;
  min-inline-size: 0;
}

.pcard-detail__label {
  flex: 0 0 auto;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  white-space: nowrap;
}

.pcard-detail__value {
  min-inline-size: 0;
  overflow: hidden;
  color: var(--text-primary);
  font-size: 0.78125rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pcard-hit {
  display: grid;
  gap: 0.4375rem;
}

.pcard-hit-label {
  display: flex;
  justify-content: space-between;
  gap: 0.5rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.pcard-hit-label b {
  color: var(--status-success);
  font-size: 0.78125rem;
  font-weight: 600;
}

.bar {
  block-size: 6px;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in srgb, var(--provider-color) 14%, transparent);
}

.bar > i {
  display: block;
  block-size: 100%;
  border-radius: 999px;
  background: color-mix(in srgb, var(--provider-color) 75%, transparent);
}

@media (prefers-reduced-motion: no-preference) {
  .bar > i {
    transition: inline-size var(--motion-base) var(--ease-out);
  }
}
</style>
