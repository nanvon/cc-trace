<script setup lang="ts">
/**
 * 状态解释。
 *
 * 紧凑面板只给一行可执行的短说明；主窗口用「标题 / 影响 / 下一步」三级结构，
 * 见 `docs/设计方向与状态规范.md` 第 7.4 节。两个变体都用同一种 tint 底色的
 * 提示条呈现：neutral 是灰调，warning／critical 才换成对应语义色，图标嵌在同色
 * 圆底里，不使用左边框色块。
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

/** 中性说明不该配警告三角：需要处理的事才用三角，其余用信息图标。 */
const needsAttention = computed(() => props.presentation.tone !== "neutral");
</script>

<template>
  <div class="alert" :class="[`alert--${variant}`, `alert--${presentation.tone}`]">
    <span class="alert__icon" aria-hidden="true">
      <svg v-if="needsAttention" viewBox="0 0 14 14" width="12" height="12" fill="none">
        <path
          d="M7 1.8 13 12.2H1z"
          stroke="currentColor"
          stroke-width="1.7"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
        <path
          d="M7 5.6v2.8M7 10.3v.1"
          stroke="currentColor"
          stroke-width="1.7"
          stroke-linecap="round"
        />
      </svg>
      <svg v-else viewBox="0 0 14 14" width="12" height="12" fill="none">
        <circle cx="7" cy="7" r="5.4" stroke="currentColor" stroke-width="1.5" />
        <path
          d="M7 4.2v.1M7 6.3v3.4"
          stroke="currentColor"
          stroke-width="1.6"
          stroke-linecap="round"
        />
      </svg>
    </span>

    <div class="alert__body">
      <template v-if="variant === 'full'">
        <p class="alert__title">{{ t(presentation.titleKey) }}</p>
        <p v-if="impact" class="alert__detail">{{ impact }}</p>
        <p v-if="nextStep" class="alert__detail alert__detail--action">{{ nextStep }}</p>
      </template>

      <p v-else class="alert__detail alert__detail--action">
        {{ nextStep ?? t(presentation.titleKey) }}
      </p>

      <p v-if="retryText" class="alert__detail numeric">{{ retryText }}</p>
    </div>
  </div>
</template>

<style scoped>
/* tint 底色的提示条：颜色始终与状态词同时出现，不单独承担语义 */
.alert {
  --tint: var(--text-secondary);
  display: flex;
  gap: 0.625rem;
  padding: var(--space-3) 0.875rem;
  background: color-mix(in srgb, var(--tint) 12%, var(--surface-raised));
  border-radius: var(--radius-small);
}

.alert__icon {
  display: grid;
  place-items: center;
  flex: 0 0 auto;
  inline-size: 1.375rem;
  block-size: 1.375rem;
  margin-block-start: 1px;
  color: var(--tint);
  background: color-mix(in srgb, var(--tint) 24%, var(--surface-raised));
  border-radius: 999px;
}

.alert__body {
  display: grid;
  gap: 0.1875rem;
  min-inline-size: 0;
}

.alert p {
  margin: 0;
}

.alert__title {
  color: var(--tint);
  font-size: 0.8125rem;
  font-weight: 700;
}

.alert__detail {
  color: var(--text-secondary);
  font-size: 0.78125rem;
  line-height: 1.55;
}

/* 下一步是这块里唯一需要读完的句子，用正文色把它从解释里提出来 */
.alert__detail--action {
  color: var(--text-primary);
}

.alert--warning {
  --tint: var(--status-warning);
}

.alert--critical {
  --tint: var(--status-error);
}
</style>
