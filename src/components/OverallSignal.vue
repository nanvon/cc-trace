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
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";

import type { ProviderSnapshot } from "../features/quota/contracts";
import { providerLabel } from "../lib/labels";
import { presentOverall, presentProvider } from "../lib/status";
import { useTimeText } from "../lib/useTimeText";

const props = defineProps<{
  providers: ProviderSnapshot[];
  variant: "compact" | "full";
  titleId?: string;
  titleTabindex?: number;
}>();

const { t } = useI18n();
const { countdown, refreshed } = useTimeText();

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

/**
 * 需要给一句说明的 Provider：非中性基调（离线／限流／过期／错误），或中性但
 * 需要引导的（无凭据、不支持）。卡片内不再承载状态说明，明细全部收在这里。
 */
const explainables = computed(() =>
  props.providers
    .map((provider) => ({ provider, presentation: presentProvider(provider) }))
    .filter(
      ({ provider, presentation }) =>
        presentation.tone !== "neutral" ||
        provider.availability === "no_credentials" ||
        provider.availability === "unsupported",
    ),
);

/** 状态点 tooltip：鼠标悬停整行（点＋刷新时间）打开，键盘 focus 状态点打开，Esc 或移开关闭。 */
const tooltipOpen = ref(false);

function retryText(provider: ProviderSnapshot): string | null {
  const remaining = countdown(provider.retryAfter);
  return remaining ? t("quota.retryIn", { time: remaining }) : null;
}
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
      <p
        class="signal__meta numeric supporting"
        :class="{ 'signal__meta--hoverable': explainables.length > 0 }"
        @mouseenter="tooltipOpen = true"
        @mouseleave="tooltipOpen = false"
      >
        <span
          class="signal__dot-wrap"
          tabindex="0"
          role="img"
          :aria-label="dotText"
          :title="dotText"
          @focus="tooltipOpen = true"
          @blur="tooltipOpen = false"
          @keydown.esc="tooltipOpen = false"
        >
          <span class="signal__dot" :class="`signal__dot--${dot}`" aria-hidden="true" />
          <div v-if="tooltipOpen && explainables.length > 0" class="signal__tooltip" role="tooltip">
            <ul class="signal__tooltip-list">
              <li
                v-for="item in explainables"
                :key="item.provider.provider"
                class="signal__tooltip-item"
              >
                <p class="signal__tooltip-name" translate="no">
                  {{ providerLabel(t, item.provider.provider) }}
                </p>
                <p class="signal__tooltip-line">{{ t(item.presentation.titleKey) }}</p>
                <p v-if="item.presentation.nextStepKey" class="signal__tooltip-line">
                  {{ t(item.presentation.nextStepKey) }}
                </p>
                <p v-if="retryText(item.provider)" class="signal__tooltip-line numeric">
                  {{ retryText(item.provider) }}
                </p>
              </li>
            </ul>
          </div>
        </span>
        <span class="signal__meta-text">{{ lastSuccess }}</span>
      </p>
    </div>

    <div class="signal__actions">
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

.signal--compact {
  gap: 0.75rem;
}

.signal--full {
  align-items: flex-start;
}

.signal__text {
  min-inline-size: 0;
}

.signal__title {
  margin: 0;
  font-weight: 600;
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
  font-size: 0.9375rem;
  font-weight: 680;
  letter-spacing: -0.01em;
  white-space: nowrap;
}

/* 窗口标题保持系统体量，不做营销型超大标题 */
.signal--full .signal__title {
  font-size: 1.4375rem;
  font-weight: 700;
  letter-spacing: -0.03em;
}

/*
 * 单行且省略：头部右侧的图标按钮组不收缩，副标题一换行就会把面板顶高
 * ——而这一行只是新鲜度，不值得为它多占一行。省略只落在文本上，
 * 整行不能 overflow hidden，否则会裁掉状态点下方的 tooltip。
 */
.signal__meta {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 1px 0 0;
  min-inline-size: 0;
  font-size: 0.6875rem;
  white-space: nowrap;
}

.signal__meta-text {
  flex: 1 1 auto;
  min-inline-size: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 有异常可看时整行才是 tooltip 热区：help 光标暗示「悬停有说明」，无异常时保持默认 */
.signal__meta--hoverable {
  cursor: help;
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

.signal--compact .signal__actions {
  gap: 2px;
}

.signal--full .signal__actions {
  gap: var(--space-2);
}

/* 状态点：6px 视觉点外包 14px 热区，margin 抵消使视觉位置不变 */
.signal__dot-wrap {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: 0 0 auto;
  inline-size: 14px;
  block-size: 14px;
  margin-inline: -4px;
  border-radius: 50%;
}

.signal__dot {
  flex: 0 0 auto;
  inline-size: 6px;
  block-size: 6px;
  background: var(--text-secondary);
  border-radius: 50%;
}

.signal__dot--live {
  background: var(--status-success);
}

.signal__dot--warning {
  background: var(--status-warning);
}

.signal__dot--critical {
  background: var(--status-error);
}

/* 状态点 tooltip：悬浮层，用面板阴影而非 lane 阴影 */
.signal__tooltip {
  position: absolute;
  inset-block-start: calc(100% + 4px);
  inset-inline-start: 0;
  z-index: 10;
  inline-size: max-content;
  max-inline-size: 15rem;
  padding: 0.5rem 0.625rem;
  color: var(--text-primary);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
  box-shadow: var(--shadow-panel);
  font-size: 0.78125rem;
  line-height: 1.5;
}

.signal__tooltip-list {
  display: grid;
  gap: 0.5rem;
  margin: 0;
  padding: 0;
  list-style: none;
}

.signal__tooltip-item {
  display: grid;
  gap: 0.125rem;
  min-inline-size: 0;
}

.signal__tooltip-name {
  margin: 0;
  font-size: 0.75rem;
  font-weight: 700;
}

.signal__tooltip-line {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.71875rem;
}
</style>
