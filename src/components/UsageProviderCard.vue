<script setup lang="ts">
/**
 * Provider 矮宽卡（ADR-0024 第 5 节）：色点＋名称 / 费用＋总 Token / 缓存命中率条。
 *
 * 输入／输出／缓存命中／缓存写入细节交还按模型表（表头已有这些列），不在卡上重复。
 * 命中率条复用余量条形态（4px），染 Provider 品牌色降饱和版，不占用语义色。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { UsageSource, UsageSummary, UsageTokenTotals } from "../features/usage/contracts";
import {
  formatUsageCost,
  formatUsagePercent,
  presentUsageTokens,
  usageCacheHitRate,
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

const providerName = computed(() => t(`provider.${props.source}`));
</script>

<template>
  <article class="pcard" :data-p="source" :aria-labelledby="titleId">
    <div class="pcard-head">
      <i aria-hidden="true"></i>
      <h3 :id="titleId">{{ providerName }}</h3>
    </div>

    <div class="pcard-money">
      <strong class="pcard-cost numeric">{{ cost ?? t("main.noValue") }}</strong>
      <span class="pcard-total numeric" :title="total.full">
        <b>{{ hasData ? total.value : t("main.noValue") }}</b
        ><small v-if="hasData && total.unit">{{ tokenUnitSeparator }}{{ total.unit }}</small>
        <span class="pcard-total__unit">{{ t("main.tokenUnit") }}</span>
      </span>
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
  gap: 0.5625rem;
  align-content: start;
  min-inline-size: 0;
  padding: 0.8125rem 0.9375rem 0.75rem;
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid color-mix(in srgb, var(--usage-divider, var(--border-subtle)) 80%, transparent);
  border-radius: 0.875rem;
  box-shadow: var(--usage-card-shadow, var(--shadow-lane));
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
  overflow: hidden;
  font-size: 0.78125rem;
  font-weight: 650;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pcard-money {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.5rem;
  min-inline-size: 0;
}

.pcard-cost {
  font-size: 1.3125rem;
  font-weight: 680;
  letter-spacing: -0.025em;
  line-height: 1;
}

.pcard-total {
  min-inline-size: 0;
  overflow: hidden;
  color: var(--text-secondary);
  font-size: 0.71875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pcard-total b {
  color: var(--text-primary);
  font-size: 0.8125rem;
  font-weight: 620;
}

.pcard-total small {
  margin-inline-start: 0.125rem;
}

.pcard-total__unit {
  margin-inline-start: 0.25rem;
}

.pcard-hit {
  display: grid;
  gap: 0.375rem;
}

.pcard-hit-label {
  display: flex;
  justify-content: space-between;
  gap: 0.5rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.pcard-hit-label b {
  color: var(--text-primary);
  font-size: 0.71875rem;
  font-weight: 620;
}

.bar {
  block-size: 4px;
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
