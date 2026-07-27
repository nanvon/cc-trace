<script setup lang="ts">
/**
 * Quota Progress —— CC Trace 的签名元素。
 *
 * 左端是数值，中间是圆角进度条，右端是重置时刻。层级靠阴影和圆角表达，
 * 正常态数值保持中性色，只有 warning／critical 才着色，
 * 见 `docs/设计方向与状态规范.md` 第 7.2 节。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import { formatAbsolute, formatPercent, formatResetClock } from "../lib/format";
import type { RailTreatment, StatusTone } from "../lib/status";

const props = withDefaults(
  defineProps<{
    /** 已本地化的窗口名。骨架状态传空字符串。 */
    label: string;
    remainingPercent: number | null;
    /** ISO 8601 UTC。 */
    resetsAt: string | null;
    treatment: RailTreatment;
    tone: StatusTone;
    /**
     * 进度条的无障碍名称，通常是「Provider 名 + 窗口名」。
     * 刻意不叫 `ariaLabel`：那会与元素自身的 `aria-label` 属性在模板里撞名。
     */
    a11yLabel: string;
    emphasis?: "primary" | "secondary";
  }>(),
  { emphasis: "secondary" },
);

const { t, locale } = useI18n();

const hasValue = computed(() => props.remainingPercent !== null && props.treatment !== "loading");

const valueText = computed(() =>
  hasValue.value && props.remainingPercent !== null
    ? formatPercent(locale.value, props.remainingPercent)
    : t("quota.noValue"),
);

/** 进度条填充比例。没有数值时为 0，且不渲染填充块。 */
const fillPercent = computed(() =>
  hasValue.value && props.remainingPercent !== null
    ? Math.max(0, Math.min(100, props.remainingPercent))
    : 0,
);

const resetText = computed(() => {
  if (!props.resetsAt) {
    return hasValue.value ? t("quota.resetsUnknown") : "";
  }
  return t("quota.resetsAt", { time: formatResetClock(locale.value, props.resetsAt) });
});

/** 相对时间必须同时提供绝对值，这里通过 title 暴露完整本地时刻。 */
const resetTitle = computed(() =>
  props.resetsAt ? formatAbsolute(locale.value, props.resetsAt) : undefined,
);

const valueTextForA11y = computed(() =>
  hasValue.value && props.remainingPercent !== null
    ? t("a11y.remaining", { percent: formatPercent(locale.value, props.remainingPercent) })
    : t("a11y.noQuota"),
);
</script>

<template>
  <div
    class="progress"
    :class="[`progress--${emphasis}`, `progress--${treatment}`, `progress--tone-${tone}`]"
  >
    <p v-if="label" class="progress__label utility-label">{{ label }}</p>

    <div class="progress__row">
      <span class="progress__value numeric">{{ valueText }}</span>

      <div
        class="progress__track"
        role="progressbar"
        :aria-label="a11yLabel"
        aria-valuemin="0"
        aria-valuemax="100"
        :aria-valuenow="hasValue ? fillPercent : undefined"
        :aria-valuetext="valueTextForA11y"
      >
        <span v-if="hasValue" class="progress__fill" :style="{ inlineSize: `${fillPercent}%` }" />
      </div>

      <span class="progress__reset numeric" :title="resetTitle">{{ resetText }}</span>
    </div>
  </div>
</template>

<style scoped>
.progress__label {
  margin-block-end: var(--space-2);
}

.progress__row {
  display: grid;
  grid-template-columns: auto minmax(3rem, 1fr) auto;
  align-items: center;
  gap: var(--space-3);
}

.progress__value {
  /* 固定最小宽度：从 5% 变到 100% 时进度条起点不左右移动 */
  min-inline-size: 3.25ch;
  font-weight: 700;
  text-align: end;
}

.progress--primary .progress__value {
  font-size: 1.5rem;
  letter-spacing: -0.01em;
}

.progress--secondary .progress__value {
  font-size: 0.9375rem;
}

/* 圆角进度条：层级靠阴影和体量表达，不是直轨道 */
.progress__track {
  position: relative;
  block-size: 0.5rem;
  background: var(--track-background);
  border-radius: 999px;
  overflow: hidden;
}

.progress--secondary .progress__track {
  block-size: 0.375rem;
}

.progress__fill {
  position: absolute;
  inset-block: 0;
  inset-inline-start: 0;
  background: var(--text-primary);
  border-radius: inherit;
}

.progress__reset {
  color: var(--text-secondary);
  font-size: 0.75rem;
  white-space: nowrap;
}

/* --- 状态处理 --- */

/* 旧快照：保留数值，但明确降级 */
.progress--faded .progress__fill {
  background: color-mix(in srgb, var(--text-primary) 38%, transparent);
}

.progress--faded .progress__value {
  color: var(--text-secondary);
}

.progress--tone-warning .progress__fill {
  background: var(--status-warning);
}

.progress--tone-warning .progress__value {
  color: var(--status-warning);
}

.progress--tone-critical .progress__fill {
  background: var(--status-error);
}

.progress--tone-critical .progress__value {
  color: var(--status-error);
}

.progress--loading .progress__track::after {
  content: "";
  position: absolute;
  inset: 0;
  background: linear-gradient(90deg, transparent, var(--border-subtle), transparent);
}

.progress--loading .progress__value,
.progress--empty .progress__value {
  color: var(--text-secondary);
}

@media (prefers-reduced-motion: no-preference) {
  .progress__fill {
    transition: inline-size var(--motion-base) var(--ease-out);
  }

  .progress--loading .progress__track::after {
    animation: progress-scan 1.5s var(--ease-out) infinite;
  }
}

@keyframes progress-scan {
  from {
    transform: translateX(-100%);
  }

  to {
    transform: translateX(100%);
  }
}
</style>
