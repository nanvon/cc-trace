<script setup lang="ts">
/**
 * Provider Lane —— 稳定的信息语法。
 *
 * 每条 lane 从身份读到剩余额度，再读到重置端点与新鲜度。失败时保持位置和既有数据，
 * 不切换成完全不同的错误卡片；无凭据时保持相同骨架，用说明替换额度区域。
 *
 * 层级靠阴影表达，不用左侧色条。百分比读数的颜色来自余量分档（ADR-0017），
 * 状态由提示条承担——两者是独立的两个维度。
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
import QuotaProgress from "./QuotaProgress.vue";
import StatusExplanation from "./StatusExplanation.vue";

const props = defineProps<{
  provider: ProviderSnapshot;
  variant: "compact" | "full";
}>();

const { t } = useI18n();

const presentation = computed(() => presentProvider(props.provider));
const name = computed(() => providerLabel(t, props.provider.provider));

/** 套餐名单独成 chip；账号是次要信息，长了就省略。 */
const plan = computed(() => props.provider.identity?.plan ?? null);
const account = computed(() => props.provider.identity?.accountHint ?? null);

const primary = computed(() => primaryWindow(props.provider.snapshot));
const secondaries = computed(() => secondaryWindows(props.provider.snapshot));

const showsRails = computed(
  () => hasQuotaValues(props.provider) || presentation.value.rail === "loading",
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
    <header class="lane__header">
      <h3 class="lane__name" translate="no">{{ name }}</h3>
      <span v-if="plan" class="chip" translate="no">{{ plan }}</span>
      <span v-if="account" class="lane__account" :title="account">{{ account }}</span>
    </header>

    <template v-if="showsRails">
      <QuotaProgress
        :label="primary ? windowLabel(t, primary) : ''"
        :remaining-percent="primary?.remainingPercent ?? null"
        :resets-at="primary?.resetsAt ?? null"
        :treatment="presentation.rail"
        :a11y-label="
          primary ? t('a11y.quotaRail', { provider: name, window: windowLabel(t, primary) }) : name
        "
        emphasis="primary"
        :size="variant"
      />

      <div v-if="secondaries.length > 0" class="lane__secondaries">
        <QuotaProgress
          v-for="window in secondaries"
          :key="window.id"
          class="lane__secondary"
          :label="windowLabel(t, window)"
          :remaining-percent="window.remainingPercent"
          :resets-at="window.resetsAt"
          :treatment="presentation.rail"
          :a11y-label="t('a11y.quotaRail', { provider: name, window: windowLabel(t, window) })"
          :size="variant"
        />
      </div>
    </template>

    <!-- 没有额度可显示时保持同一骨架：读数位置留占位符，不伪造 0% -->
    <p v-else class="lane__blank numeric" :class="`lane__blank--${variant}`">
      {{ t("quota.noValue") }}
    </p>

    <StatusExplanation
      v-if="showsExplanation"
      :provider="provider"
      :presentation="presentation"
      :variant="variant"
    />
  </article>
</template>

<style scoped>
/* 层级靠阴影和圆角表达，不用左侧色条 */
.lane {
  display: grid;
  gap: var(--space-3);
  align-content: start;
  min-inline-size: 0;
  padding: var(--space-3) 0.8125rem;
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  box-shadow: var(--shadow-lane);
}

.lane--full {
  gap: var(--space-4);
  padding: 1.125rem var(--space-5);
}

.lane__header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-inline-size: 0;
}

.lane__name {
  margin: 0;
  font-size: 0.875rem;
  font-weight: 650;
  letter-spacing: -0.008em;
  white-space: nowrap;
}

.lane--full .lane__name {
  font-size: 1.0625rem;
  letter-spacing: -0.014em;
}

/* 账号靠右，长账号名省略而不换行，也不挤压 Provider 名 */
.lane__account {
  margin-inline-start: auto;
  max-inline-size: 42%;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.lane--full .lane__account {
  max-inline-size: 45%;
  font-size: 0.8125rem;
}

/* 专项额度与主额度之间用虚线分隔：同一 Provider 内部的次级信息，不是新的分区 */
.lane__secondaries {
  display: grid;
  gap: var(--space-2);
}

.lane__secondary {
  padding-block-start: var(--space-2);
  border-block-start: 1px dashed var(--border-subtle);
}

.lane__blank {
  margin: 0;
  color: var(--text-secondary);
  font-size: 1.4375rem;
  font-weight: 700;
  line-height: 1;
  letter-spacing: -0.03em;
}

.lane__blank--full {
  font-size: 1.9375rem;
  letter-spacing: -0.035em;
}
</style>
