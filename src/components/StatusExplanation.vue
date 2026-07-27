<script setup lang="ts">
/**
 * 状态解释。
 *
 * 紧凑面板只给一行可执行的短说明；主窗口用「标题 / 影响 / 下一步」三级结构，
 * 见 `docs/设计方向与状态规范.md` 第 7.4 节。两个变体都用同一种 tint 底色的
 * 提示条呈现：neutral 是灰调，warning／critical 才换成对应语义色。
 *
 * 文案永远来自 `lib/status.ts` 给出的 key：这里不重新判断状态。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { ProviderSnapshot } from "../features/quota/contracts";
import type { StatusPresentation } from "../lib/status";
import { useTimeText } from "../lib/useTimeText";

const props = defineProps<{
  provider: ProviderSnapshot;
  presentation: StatusPresentation;
  variant: "compact" | "full";
}>();

const { t } = useI18n();
const { past, countdown } = useTimeText();

const impact = computed(() => {
  if (!props.presentation.impactKey) {
    return null;
  }
  return t(props.presentation.impactKey, { time: past(props.provider.lastSuccessAt) });
});

/** 退避期内必须给出可再次尝试的时间，即使用户刚刚手动点过刷新。 */
const retryText = computed(() => {
  const remaining = countdown(props.provider.retryAfter);
  return remaining ? t("quota.retryIn", { time: remaining }) : null;
});

const nextStep = computed(() =>
  props.presentation.nextStepKey ? t(props.presentation.nextStepKey) : null,
);
</script>

<template>
  <div
    class="explanation"
    :class="[`explanation--${variant}`, `explanation--${presentation.tone}`]"
  >
    <template v-if="variant === 'full'">
      <p class="explanation__title">{{ t(presentation.titleKey) }}</p>
      <p v-if="impact" class="explanation__impact supporting">{{ impact }}</p>
      <p v-if="nextStep" class="explanation__next">{{ nextStep }}</p>
      <p v-if="retryText" class="explanation__retry numeric supporting">{{ retryText }}</p>
    </template>

    <template v-else>
      <p class="explanation__next">{{ nextStep ?? t(presentation.titleKey) }}</p>
      <p v-if="retryText" class="explanation__retry numeric supporting">{{ retryText }}</p>
    </template>
  </div>
</template>

<style scoped>
/* tint 底色的提示条：neutral 是灰调，warning/critical 换成对应语义色，
   颜色始终与状态词同时出现，不单独承担语义 */
.explanation {
  --tint: var(--text-secondary);
  display: grid;
  gap: var(--space-1);
  padding: var(--space-3);
  background: color-mix(in srgb, var(--tint) 10%, var(--surface-raised));
  border-radius: var(--radius-small);
}

.explanation--full {
  gap: var(--space-2);
  padding: var(--space-4);
}

.explanation p {
  margin: 0;
}

.explanation__title {
  font-weight: 600;
  color: var(--tint);
}

.explanation__impact,
.explanation__next {
  line-height: 1.55;
}

.explanation--compact .explanation__next {
  color: var(--text-secondary);
  font-size: 0.8125rem;
}

.explanation__retry {
  font-size: 0.75rem;
}

.explanation--warning {
  --tint: var(--status-warning);
}

.explanation--critical {
  --tint: var(--status-error);
}
</style>
