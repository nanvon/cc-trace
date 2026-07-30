<script setup lang="ts">
/**
 * 总体状态：只回答「现在最需要注意什么」。
 *
 * 它提高最高风险 Provider 的视觉权重，但**不改变 Provider 的空间顺序**；
 * 没有风险时也不显示夸张的绿色成功横幅，见 `docs/设计方向与状态规范.md` 第 7.3 节。
 *
 * 两个变体的标题都是稳定的表面名，不是状态句：状态由状态点和每条 lane 的提示条
 * 承担。稳定标题也是焦点恢复时要播报的那一个，见第 7.6 节。
 *
 * 状态点有可访问名称，因此颜色不是状态的唯一载体（第 3.3 节）。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { ProviderSnapshot } from "../features/quota/contracts";
import { presentOverall } from "../lib/status";
import { useTimeText } from "../lib/useTimeText";

const props = defineProps<{
  providers: ProviderSnapshot[];
  variant: "compact" | "full";
  titleId?: string;
  titleTabindex?: number;
}>();

const { t } = useI18n();
const { refreshed } = useTimeText();

const leader = computed(() => presentOverall(props.providers));

/** 所有 Provider 都是当前实时数据。 */
const allHealthy = computed(
  () =>
    props.providers.length > 0 &&
    props.providers.every(
      (provider) => provider.freshness === "live" && provider.availability === "ready",
    ),
);

const title = computed(() => (props.variant === "full" ? t("main.title") : t("compact.title")));

const tone = computed(() =>
  allHealthy.value ? "neutral" : (leader.value?.presentation.tone ?? "neutral"),
);

/**
 * 状态点。只有「全部实时」才亮绿并带呼吸，其余按风险着色；
 * 没有任何数据时是静默的灰点，不假装连接正常。
 */
const dot = computed(() => {
  if (allHealthy.value) {
    return "live";
  }
  if (tone.value !== "neutral") {
    return tone.value;
  }
  return "idle";
});

/** 状态点的可读名称。有它，颜色就不是状态的唯一载体。 */
const dotText = computed(() => {
  switch (dot.value) {
    case "live":
      return t("compact.signal.live");
    case "warning":
      return t("compact.signal.stale");
    case "critical":
      return t("compact.signal.attention");
    default:
      return t("compact.signal.idle");
  }
});

/** 最近一次成功刷新，取两个 Provider 里更新的那个。 */
const lastSuccess = computed(() => {
  const timestamps = props.providers
    .map((provider) => provider.lastSuccessAt)
    .filter((value): value is string => value !== null)
    .sort();
  return refreshed(timestamps.at(-1) ?? null);
});
</script>

<template>
  <div class="signal" :class="`signal--${variant}`">
    <div class="signal__text">
      <component
        :is="variant === 'full' ? 'h1' : 'p'"
        :id="titleId"
        class="signal__title"
        :tabindex="titleTabindex"
      >
        {{ title }}
      </component>
      <p class="signal__meta numeric supporting">{{ lastSuccess }}</p>
    </div>

    <div class="signal__actions">
      <span
        class="signal__dot"
        :class="`signal__dot--${dot}`"
        role="img"
        :aria-label="dotText"
        :title="dotText"
      />
      <slot name="actions" />
    </div>
  </div>
</template>

<style scoped>
.signal {
  display: flex;
  align-items: center;
  gap: var(--space-4);
}

.signal--full {
  align-items: flex-start;
}

.signal__text {
  min-inline-size: 0;
}

.signal__title {
  margin: 0;
  font-weight: 700;
  letter-spacing: -0.01em;
}

/*
 * 紧凑面板打开时用标题承接程序化焦点并播报当前表面。
 * 标题不在 Tab 顺序里，也不是操作，因此不画会被误解成选中态的焦点环；
 * 用户按 Tab 后，真正的按钮仍使用全局 :focus-visible。
 */
.signal__title[tabindex="-1"]:focus {
  outline: none;
}

.signal--compact .signal__title {
  font-size: 0.875rem;
  white-space: nowrap;
}

/* 窗口标题保持系统体量，不做营销型超大标题 */
.signal--full .signal__title {
  font-size: 1.4375rem;
  letter-spacing: -0.03em;
}

/*
 * 单行且省略：头部右侧的图标按钮组不收缩，副标题一换行就会把面板顶高
 * ——而这一行只是新鲜度，不值得为它多占一行。
 */
.signal__meta {
  margin: 1px 0 0;
  overflow: hidden;
  font-size: 0.75rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.signal--full .signal__meta {
  margin-block-start: var(--space-1);
  font-size: 0.8125rem;
}

.signal__actions {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  gap: var(--space-1);
  margin-inline-start: auto;
}

.signal--full .signal__actions {
  gap: var(--space-2);
}

.signal__dot {
  flex: 0 0 auto;
  inline-size: 7px;
  block-size: 7px;
  margin-inline-end: var(--space-1);
  background: var(--text-secondary);
  border-radius: 50%;
}

.signal__dot--live {
  background: var(--status-success);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--status-success) 22%, transparent);
}

.signal__dot--warning {
  background: var(--status-warning);
}

.signal__dot--critical {
  background: var(--status-error);
}

@media (prefers-reduced-motion: no-preference) {
  /* 只有「一切正常」才呼吸：它是唯一不需要用户读文字的状态 */
  .signal__dot--live {
    animation: signal-pulse 2.4s var(--ease-out) infinite;
  }
}

@keyframes signal-pulse {
  0%,
  100% {
    opacity: 1;
  }

  50% {
    opacity: 0.55;
  }
}
</style>
