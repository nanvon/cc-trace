<script setup lang="ts">
/**
 * 主额度轨道右下角的今日／本周费用。
 *
 * 这里只负责展示 Rust 已完成聚合的可计算金额，不在组件内重算 Token 或价格。
 * 扫描中保留上一份完成结果；没有任何可计算金额时显示占位符，不伪装成 `$0`。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { UsagePeriodCost, UsageProviderCosts } from "../features/usage/contracts";
import { presentUsageCost } from "../features/usage/presentation";
import { formatCompactUsdNanos, formatUsdNanos } from "../lib/format";

const props = withDefaults(
  defineProps<{
    providerName: string;
    costs: UsageProviderCosts;
    scanning?: boolean;
  }>(),
  { scanning: false },
);

const { t, locale } = useI18n();

interface PeriodPresentation {
  key: "today" | "week";
  label: string;
  visible: string;
  description: string;
}

function periodPresentation(
  key: PeriodPresentation["key"],
  label: string,
  cost: UsagePeriodCost | null,
): PeriodPresentation {
  const display = presentUsageCost(cost);
  if (display.amountNanos === null) {
    const allUnpriced =
      cost !== null && cost.entryCount > 0 && cost.pricedEntries === 0 && cost.unpricedEntries > 0;
    return {
      key,
      label,
      visible: "—",
      description: t(allUnpriced ? "compact.usage.amountUnpriced" : "compact.usage.amountPending", {
        period: label,
      }),
    };
  }

  const compact = formatCompactUsdNanos(locale.value, display.amountNanos);
  const full = formatUsdNanos(locale.value, display.amountNanos);
  return {
    key,
    label,
    visible: compact,
    description: t("compact.usage.amountExact", { period: label, amount: full }),
  };
}

const periods = computed<PeriodPresentation[]>(() => [
  periodPresentation("today", t("compact.usage.today"), props.costs.today),
  periodPresentation("week", t("compact.usage.thisWeek"), props.costs.week),
]);
</script>

<template>
  <div
    class="usage-cost"
    role="group"
    :aria-label="t('a11y.apiEquivalentCosts', { provider: providerName })"
  >
    <dl class="usage-cost__periods">
      <div v-for="period in periods" :key="period.key" class="usage-cost__period">
        <dt>{{ period.label }}</dt>
        <dd
          class="usage-cost__amount numeric"
          :title="period.description"
          :aria-label="period.description"
        >
          {{ period.visible }}
        </dd>
      </div>
    </dl>

    <span v-if="scanning" class="usage-cost__loading" aria-hidden="true">
      <svg viewBox="0 0 12 12" width="10" height="10" fill="none">
        <circle
          cx="6"
          cy="6"
          r="4.25"
          stroke="currentColor"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-dasharray="18 9"
        />
      </svg>
    </span>
  </div>
</template>

<style scoped>
/* 原型评审稿 lane-costs：label–value 成对内联，两端对齐 */
.usage-cost {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.625rem;
  min-inline-size: 0;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.usage-cost__periods {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 1px 0.625rem;
  min-inline-size: 0;
  margin: 0;
}

.usage-cost__period {
  display: inline-flex;
  align-items: baseline;
  gap: 5px;
  min-inline-size: 0;
  overflow: hidden;
}

.usage-cost__period dt {
  white-space: nowrap;
}

.usage-cost__amount {
  min-inline-size: 0;
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-primary);
  font-size: 0.78125rem;
  font-weight: 620;
  line-height: 1;
}

.usage-cost__loading {
  display: grid;
  flex: 0 0 0.625rem;
  inline-size: 0.625rem;
  block-size: 0.625rem;
  place-items: center;
  color: var(--text-secondary);
  opacity: 0.65;
}

.usage-cost__loading svg {
  display: block;
  transform-box: fill-box;
  transform-origin: center;
}

@media (prefers-reduced-motion: no-preference) {
  .usage-cost__loading svg {
    animation: usage-cost-loading 900ms linear infinite;
  }
}

@keyframes usage-cost-loading {
  to {
    rotate: 360deg;
  }
}
</style>
