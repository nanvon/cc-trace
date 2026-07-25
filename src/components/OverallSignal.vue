<script setup lang="ts">
/**
 * 总体状态：只回答「现在最需要注意什么」。
 *
 * 它提高最高风险 Provider 的视觉权重，但**不改变 Provider 的空间顺序**；
 * 没有风险时也不显示夸张的绿色成功横幅，见 `docs/设计方向与状态规范.md` 第 7.3 节。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { ProviderSnapshot } from "../features/quota/contracts";
import { providerLabel } from "../lib/labels";
import { presentOverall } from "../lib/status";
import { useTimeText } from "../lib/useTimeText";

const props = defineProps<{
  providers: ProviderSnapshot[];
  variant: "compact" | "full";
}>();

const { t } = useI18n();
const { past } = useTimeText();

const leader = computed(() => presentOverall(props.providers));

/** 所有 Provider 都是当前实时数据。 */
const allHealthy = computed(
  () =>
    props.providers.length > 0 &&
    props.providers.every(
      (provider) => provider.freshness === "live" && provider.availability === "ready",
    ),
);

const title = computed(() => {
  if (allHealthy.value) {
    return t("overall.allHealthy");
  }
  if (!leader.value) {
    return t("status.loading");
  }
  return t("overall.focus", {
    provider: providerLabel(t, leader.value.provider.provider),
    status: t(leader.value.presentation.titleKey),
  });
});

const tone = computed(() =>
  allHealthy.value ? "neutral" : (leader.value?.presentation.tone ?? "neutral"),
);

/** 最近一次成功刷新，取两个 Provider 里更新的那个。 */
const lastSuccess = computed(() => {
  const timestamps = props.providers
    .map((provider) => provider.lastSuccessAt)
    .filter((value): value is string => value !== null)
    .sort();
  const latest = timestamps.at(-1) ?? null;
  return latest ? t("quota.lastSuccess", { time: past(latest) }) : t("quota.neverRefreshed");
});
</script>

<template>
  <div class="signal" :class="[`signal--${variant}`, `signal--${tone}`]">
    <div class="signal__text">
      <component :is="variant === 'full' ? 'h1' : 'p'" class="signal__title">
        {{ title }}
      </component>
      <p class="signal__meta numeric supporting">{{ lastSuccess }}</p>
    </div>

    <div class="signal__actions">
      <slot name="actions" />
    </div>
  </div>
</template>

<style scoped>
.signal {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-4);
}

.signal__text {
  min-inline-size: 0;
}

.signal__title {
  margin: 0;
  font-weight: 620;
  letter-spacing: -0.015em;
}

.signal--compact .signal__title {
  font-size: 0.9375rem;
}

/* 窗口标题保持系统体量，不做营销型超大标题 */
.signal--full .signal__title {
  font-size: 1.75rem;
  letter-spacing: -0.025em;
}

.signal__meta {
  margin: var(--space-1) 0 0;
  font-size: 0.75rem;
}

.signal--full .signal__meta {
  margin-top: var(--space-2);
  font-size: 0.8125rem;
}

.signal--warning .signal__title {
  color: var(--status-warning);
}

.signal--critical .signal__title {
  color: var(--status-error);
}

.signal__actions {
  display: flex;
  flex-shrink: 0;
  gap: var(--space-2);
}
</style>
