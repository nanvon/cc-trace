<script setup lang="ts">
/**
 * Provider Lane —— 稳定的信息语法。
 *
 * 每条 lane 从身份读到剩余额度，再读到重置端点与新鲜度。失败时保持位置和既有数据，
 * 不切换成完全不同的错误卡片；无凭据时保持相同骨架，用说明替换额度区域。
 *
 * 左侧 spine 只在异常时着色：风险改变强调，不改变空间顺序。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import {
  primaryWindow,
  secondaryWindows,
  type ProviderSnapshot,
} from "../features/quota/contracts";
import { providerLabel, windowLabel } from "../lib/labels";
import { hasQuotaValues, presentProvider } from "../lib/status";
import { useTimeText } from "../lib/useTimeText";
import ResetRail from "./ResetRail.vue";
import StatusExplanation from "./StatusExplanation.vue";

const props = defineProps<{
  provider: ProviderSnapshot;
  variant: "compact" | "full";
}>();

const { t } = useI18n();
const { past } = useTimeText();

const presentation = computed(() => presentProvider(props.provider));
const name = computed(() => providerLabel(t, props.provider.provider));

const identity = computed(() => {
  const value = props.provider.identity;
  if (!value) {
    return null;
  }
  const parts = [
    value.plan ? t("provider.plan", { plan: value.plan }) : null,
    value.accountHint ? t("provider.account", { hint: value.accountHint }) : null,
  ].filter((part): part is string => part !== null);
  return parts.length > 0 ? parts.join(" · ") : null;
});

const primary = computed(() => primaryWindow(props.provider.snapshot));
const secondaries = computed(() =>
  props.variant === "full" ? secondaryWindows(props.provider.snapshot) : [],
);

const showsRails = computed(
  () => hasQuotaValues(props.provider) || presentation.value.rail === "loading",
);

/** 有数值时才谈「上次成功刷新」；没有数值时这一行会误导。 */
const lastSuccess = computed(() =>
  props.provider.lastSuccessAt
    ? t("quota.lastSuccess", { time: past(props.provider.lastSuccessAt) })
    : t("quota.neverRefreshed"),
);

/** 紧凑面板：有数值时状态词已经说明一切，不再叠加一行解释。 */
const showsExplanation = computed(() =>
  props.variant === "full"
    ? presentation.value.impactKey !== null
    : !showsRails.value || presentation.value.tone !== "neutral",
);
</script>

<template>
  <article class="lane" :class="[`lane--${variant}`, `lane--${presentation.tone}`]">
    <span class="lane__spine" aria-hidden="true" />

    <div class="lane__body">
      <header class="lane__header">
        <div class="lane__identity">
          <h3 class="lane__name" translate="no">{{ name }}</h3>
          <p v-if="identity && variant === 'full'" class="lane__account supporting">
            {{ identity }}
          </p>
        </div>

        <div class="lane__meta">
          <span class="lane__status">{{ t(presentation.titleKey) }}</span>
          <span class="lane__time numeric supporting">{{ lastSuccess }}</span>
        </div>
      </header>

      <div v-if="showsRails" class="lane__rails">
        <ResetRail
          :label="primary && variant === 'full' ? windowLabel(t, primary) : ''"
          :remaining-percent="primary?.remainingPercent ?? null"
          :resets-at="primary?.resetsAt ?? null"
          :treatment="presentation.rail"
          :tone="presentation.tone"
          :a11y-label="
            primary
              ? t('a11y.quotaRail', { provider: name, window: windowLabel(t, primary) })
              : name
          "
          emphasis="primary"
        />

        <ResetRail
          v-for="window in secondaries"
          :key="window.id"
          :label="windowLabel(t, window)"
          :remaining-percent="window.remainingPercent"
          :resets-at="window.resetsAt"
          :treatment="presentation.rail"
          :tone="presentation.tone"
          :a11y-label="t('a11y.quotaRail', { provider: name, window: windowLabel(t, window) })"
        />
      </div>

      <StatusExplanation
        v-if="showsExplanation"
        :provider="provider"
        :presentation="presentation"
        :variant="variant"
      />
    </div>
  </article>
</template>

<style scoped>
.lane {
  display: grid;
  grid-template-columns: 2px minmax(0, 1fr);
  gap: var(--space-4);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  overflow: hidden;
}

.lane__spine {
  background: var(--border-subtle);
}

.lane--warning .lane__spine {
  background: var(--status-warning);
}

.lane--critical .lane__spine {
  background: var(--status-error);
}

.lane__body {
  display: grid;
  gap: var(--space-4);
  min-inline-size: 0;
  padding: var(--space-4) var(--space-4) var(--space-4) 0;
}

.lane--full .lane__body {
  gap: var(--space-5);
  padding-block: var(--space-5);
  padding-inline-end: var(--space-5);
}

.lane__header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--space-3);
}

.lane__identity {
  min-inline-size: 0;
}

.lane__name {
  margin: 0;
  font-size: 0.9375rem;
  font-weight: 620;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.lane--full .lane__name {
  font-size: 1.0625rem;
}

.lane__account {
  margin: var(--space-1) 0 0;
  font-size: 0.75rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.lane__meta {
  display: grid;
  gap: 2px;
  justify-items: end;
  text-align: end;
}

.lane__status {
  font-size: 0.75rem;
  font-weight: 560;
}

.lane--warning .lane__status {
  color: var(--status-warning);
}

.lane--critical .lane__status {
  color: var(--status-error);
}

.lane__time {
  font-size: 0.6875rem;
}

.lane__rails {
  display: grid;
  gap: var(--space-4);
}

.lane--compact .lane__rails {
  gap: var(--space-3);
}
</style>
