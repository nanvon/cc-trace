<script setup lang="ts">
/**
 * Quota Progress —— CC Trace 的签名元素。
 *
 * 两种排布，见 `docs/设计方向与状态规范.md` 第 7.2 节：
 * - `primary`：读数行（大百分比 + 窗口名 + 重置时间）在上，全宽进度条在下。
 * - `secondary`：单行（窗口名 + 细进度条 + 百分比 + 重置时间）。
 *
 * 颜色由**余量基调**驱动（`lib/quotaTone.ts`），不是状态基调：状态基调负责提示条与
 * 状态词，余量基调负责这里的读数与填充，见 ADR-0017。快照不新鲜时自动降级为中性。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import { formatAbsolute, formatPercent, formatResetClock } from "../lib/format";
import { displayQuotaTone } from "../lib/quotaTone";
import type { RailTreatment } from "../lib/status";

const props = withDefaults(
  defineProps<{
    /** 已本地化的窗口名。`primary` 排布里它出现在读数右侧。 */
    label: string;
    remainingPercent: number | null;
    /** ISO 8601 UTC。 */
    resetsAt: string | null;
    treatment: RailTreatment;
    /**
     * 进度条的无障碍名称，通常是「Provider 名 + 窗口名」。
     * 刻意不叫 `ariaLabel`：那会与元素自身的 `aria-label` 属性在模板里撞名。
     */
    a11yLabel: string;
    emphasis?: "primary" | "secondary";
    /** 紧凑面板与主窗口的读数字号差异。 */
    size?: "compact" | "full";
  }>(),
  { emphasis: "secondary", size: "compact" },
);

const { t, locale } = useI18n();

const hasValue = computed(() => props.remainingPercent !== null && props.treatment !== "loading");

const tone = computed(() => displayQuotaTone(props.remainingPercent, props.treatment));

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
    :class="[
      `progress--${emphasis}`,
      `progress--${size}`,
      `progress--${treatment}`,
      `progress--tone-${tone}`,
    ]"
  >
    <!-- primary：读数在上、全宽轨道在下。窗口名与重置时间同一行，不遮蔽彼此 -->
    <template v-if="emphasis === 'primary'">
      <div class="progress__reading">
        <span class="progress__value numeric">{{ valueText }}</span>
        <span v-if="label" class="progress__window">{{ label }}</span>
        <span class="progress__reset numeric" :title="resetTitle">{{ resetText }}</span>
      </div>

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
    </template>

    <!-- secondary：单行，轨道在中间吸收剩余宽度 -->
    <div v-else class="progress__row">
      <span class="progress__window">{{ label }}</span>

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

      <span class="progress__value numeric">{{ valueText }}</span>
      <span class="progress__reset numeric" :title="resetTitle">{{ resetText }}</span>
    </div>
  </div>
</template>

<style scoped>
.progress__reading {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  margin-block-end: var(--space-3);
}

.progress__row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.progress__value {
  font-weight: 700;
}

.progress--primary .progress__value {
  font-size: 1.4375rem;
  line-height: 1;
  letter-spacing: -0.03em;
}

.progress--primary.progress--full .progress__value {
  font-size: 1.9375rem;
  letter-spacing: -0.035em;
}

.progress--secondary .progress__value {
  flex: 0 0 auto;
  font-size: 0.71875rem;
  font-weight: 650;
}

.progress--secondary.progress--full .progress__value {
  font-size: 0.8125rem;
}

.progress__window {
  color: var(--text-secondary);
  font-size: 0.75rem;
}

.progress--full .progress__window {
  font-size: 0.8125rem;
}

.progress--secondary .progress__window {
  /* 窗口名不参与压缩：先让轨道让位，长模型名再省略 */
  flex: 0 1 auto;
  font-size: 0.71875rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 重置时间推到行尾，与读数保持在同一基线上 */
.progress__reading .progress__reset {
  margin-inline-start: auto;
}

.progress__reset {
  color: var(--text-secondary);
  font-size: 0.75rem;
  white-space: nowrap;
}

.progress--secondary .progress__reset {
  flex: 0 0 auto;
  font-size: 0.6875rem;
}

/* 圆角进度条：层级靠体量表达，不是直轨道 */
.progress__track {
  position: relative;
  block-size: 0.5rem;
  background: var(--track-background);
  border-radius: 999px;
  overflow: hidden;
}

.progress--secondary .progress__track {
  flex: 1 1 auto;
  min-inline-size: 1.75rem;
  block-size: 0.25rem;
}

.progress__fill {
  position: absolute;
  inset-block: 0;
  inset-inline-start: 0;
  background: var(--text-primary);
  border-radius: inherit;
}

/* --- 余量分档。颜色只跟随剩余百分比，不跟随可用性状态 --- */

.progress--tone-warning {
  --quota-tone: var(--status-warning);
}

.progress--tone-low {
  --quota-tone: var(--status-low);
}

.progress--tone-danger {
  --quota-tone: var(--status-error);
}

.progress--tone-warning .progress__fill,
.progress--tone-low .progress__fill,
.progress--tone-danger .progress__fill {
  background: var(--quota-tone);
}

.progress--tone-warning .progress__value,
.progress--tone-low .progress__value,
.progress--tone-danger .progress__value {
  color: var(--quota-tone);
}

/* --- 新鲜度处理。旧快照保留数值，但不允许用余量色宣称当前紧张 --- */

.progress--faded .progress__fill {
  background: color-mix(in srgb, var(--text-primary) 38%, transparent);
}

.progress--faded .progress__value,
.progress--loading .progress__value,
.progress--empty .progress__value {
  color: var(--text-secondary);
}

.progress--loading .progress__track::after {
  content: "";
  position: absolute;
  inset: 0;
  background: linear-gradient(90deg, transparent, var(--border-subtle), transparent);
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
