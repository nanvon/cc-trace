<script setup lang="ts">
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

const providerName = computed(() => t(`provider.${props.source}`));

function token(value: number) {
  return presentUsageTokens(locale.value, value);
}

function valueOrDash(value: number): string {
  if (!hasData.value) return t("main.noValue");
  const display = token(value);
  const separator = display.unit && !locale.value.toLowerCase().startsWith("zh") ? " " : "";
  return `${display.value}${separator}${display.unit}`;
}
</script>

<template>
  <article class="usage-provider" :data-provider="source" :aria-labelledby="titleId">
    <header class="usage-provider__header">
      <span class="usage-provider__marker" aria-hidden="true"></span>
      <h3 :id="titleId">{{ providerName }}</h3>
    </header>

    <div class="usage-provider__money">
      <strong class="numeric">{{ cost ?? t("main.noValue") }}</strong>
      <span class="usage-provider__total numeric" :title="total.full">
        {{ hasData ? total.value : t("main.noValue")
        }}<small v-if="hasData && total.unit">{{ tokenUnitSeparator }}{{ total.unit }}</small>
        {{ t("main.tokenUnit") }}
      </span>
    </div>

    <dl class="usage-provider__metrics">
      <div>
        <dt>{{ t("main.input") }}</dt>
        <dd class="numeric">{{ valueOrDash(tokens.inputTokens) }}</dd>
      </div>
      <div>
        <dt>{{ t("main.output") }}</dt>
        <dd class="numeric">{{ valueOrDash(tokens.outputTokens) }}</dd>
      </div>
      <div>
        <dt>{{ t("main.cacheHit") }}</dt>
        <dd class="numeric">{{ valueOrDash(tokens.cacheReadInputTokens) }}</dd>
      </div>
      <div>
        <dt>{{ t("main.cacheWrite") }}</dt>
        <dd class="numeric">{{ valueOrDash(usageCacheWriteTokens(tokens)) }}</dd>
      </div>
    </dl>

    <div class="usage-provider__hit">
      <div class="usage-provider__hit-label">
        <span>{{ t("main.cacheHitRate") }}</span>
        <strong class="numeric">
          {{ hasData ? formatUsagePercent(locale, hitRate) : t("main.noValue") }}
        </strong>
      </div>
      <div class="usage-provider__track" aria-hidden="true">
        <span :style="{ width: `${hasData ? (hitRate ?? 0) : 0}%` }"></span>
      </div>
    </div>
  </article>
</template>

<style scoped>
.usage-provider {
  --provider-color: var(--cat-codex);
  --provider-tint: var(--cat-codex-dim);
  padding: 0.9375rem 1rem 0.875rem;
  background: var(--usage-surface, var(--surface-raised));
  border: 1px solid var(--border-subtle);
  border-radius: 0.625rem;
  box-shadow: var(--usage-card-shadow, var(--shadow-lane));
}

.usage-provider[data-provider="claude"] {
  --provider-color: var(--cat-claude);
  --provider-tint: var(--cat-claude-dim);
}

.usage-provider__header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-block-end: 0.75rem;
}

.usage-provider__header h3 {
  margin: 0;
  font-size: 0.6875rem;
  font-weight: 680;
}

.usage-provider__marker {
  inline-size: 0.4375rem;
  block-size: 0.4375rem;
  flex: 0 0 auto;
  border-radius: 0.125rem;
  background: var(--provider-color);
}

.usage-provider__money {
  display: flex;
  align-items: baseline;
  gap: 1.125rem;
  margin-block-end: 0.8125rem;
}

.usage-provider__money strong {
  font-size: 1.375rem;
  font-weight: 690;
  letter-spacing: -0.025em;
  line-height: 1;
}

.usage-provider__total {
  color: var(--text-secondary);
  font-size: 0.65625rem;
}

.usage-provider__total small {
  margin-inline-start: 0.125rem;
  font-size: 0.625rem;
}

.usage-provider__metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0.5625rem;
  margin: 0;
}

.usage-provider__metrics div {
  min-inline-size: 0;
}

.usage-provider__metrics dt,
.usage-provider__metrics dd {
  margin: 0;
}

.usage-provider__metrics dt {
  color: var(--text-secondary);
  font-size: 0.59375rem;
  white-space: nowrap;
}

.usage-provider__metrics dd {
  margin-block-start: 0.125rem;
  overflow: hidden;
  font-size: 0.71875rem;
  font-weight: 610;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.usage-provider__hit {
  margin-block-start: 0.75rem;
  padding-block-start: 0.625rem;
  border-block-start: 1px solid var(--border-subtle);
}

.usage-provider__hit-label {
  display: flex;
  justify-content: space-between;
  gap: var(--space-3);
  margin-block-end: 0.3125rem;
  color: var(--text-secondary);
  font-size: 0.59375rem;
}

.usage-provider__hit-label strong {
  color: var(--text-primary);
  font-size: 0.65625rem;
  font-weight: 650;
}

.usage-provider__track {
  block-size: 0.3125rem;
  overflow: hidden;
  border-radius: 999px;
  background: var(--provider-tint);
}

.usage-provider__track span {
  display: block;
  block-size: 100%;
  border-radius: inherit;
  background: var(--provider-color);
}

@media (prefers-reduced-motion: no-preference) {
  .usage-provider__track span {
    transition: width var(--motion-base) var(--ease-out);
  }
}

@media (max-width: 820px) {
  .usage-provider__money {
    gap: 0.75rem;
  }
}
</style>
