<script setup lang="ts">
/**
 * Reset Rail —— CC Trace 的签名元素。
 *
 * 一条直轨道把「剩余多少」和「何时重置」放进同一条阅读路径：
 * 左端是数值，中间是带刻度的轨道，右端是终点标记与重置时刻。
 *
 * 它不是进度条：轨道是直的、端点只做轻微软化、右端有明确的终点标记，
 * 不使用圆形 Gauge、速度表或动画计数，见 `DESIGN.md` 的 Shapes 段。
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
     * 轨道的无障碍名称，通常是「Provider 名 + 窗口名」。
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

/** 轨道填充比例。没有数值时为 0，且不渲染填充块。 */
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
  <div class="rail" :class="[`rail--${emphasis}`, `rail--${treatment}`, `rail--tone-${tone}`]">
    <p v-if="label" class="rail__label utility-label">{{ label }}</p>

    <div class="rail__row">
      <span class="rail__value numeric">{{ valueText }}</span>

      <div
        class="rail__track"
        role="progressbar"
        :aria-label="a11yLabel"
        aria-valuemin="0"
        aria-valuemax="100"
        :aria-valuenow="hasValue ? fillPercent : undefined"
        :aria-valuetext="valueTextForA11y"
      >
        <span v-if="hasValue" class="rail__fill" :style="{ inlineSize: `${fillPercent}%` }" />
        <span class="rail__tick" style="inset-inline-start: 25%" aria-hidden="true" />
        <span class="rail__tick" style="inset-inline-start: 50%" aria-hidden="true" />
        <span class="rail__tick" style="inset-inline-start: 75%" aria-hidden="true" />
        <span class="rail__terminal" aria-hidden="true" />
      </div>

      <span class="rail__reset numeric" :title="resetTitle">{{ resetText }}</span>
    </div>
  </div>
</template>

<style scoped>
.rail__label {
  margin-block-end: var(--space-2);
}

.rail__row {
  display: grid;
  grid-template-columns: auto minmax(3rem, 1fr) auto;
  align-items: center;
  gap: var(--space-3);
}

.rail__value {
  /* 固定最小宽度：从 5% 变到 100% 时轨道起点不左右移动 */
  min-inline-size: 3.25ch;
  font-weight: 700;
  text-align: end;
}

.rail--primary .rail__value {
  font-size: 1.5rem;
  letter-spacing: -0.02em;
}

.rail--secondary .rail__value {
  font-size: 0.9375rem;
}

.rail__track {
  position: relative;
  block-size: var(--rail-height);
  background: var(--track-background);
  border-radius: var(--rail-radius);
}

.rail__fill {
  position: absolute;
  inset-block: 0;
  inset-inline-start: 0;
  background: var(--text-primary);
  border-radius: inherit;
}

/* 刻度让它读起来是量规轨道，而不是加载条 */
.rail__tick {
  position: absolute;
  inset-block-start: -2px;
  inline-size: 1px;
  block-size: calc(var(--rail-height) + 4px);
  background: var(--rail-tick);
}

/* 终点标记：轨道到此为止，右侧紧跟重置时刻 */
.rail__terminal {
  position: absolute;
  inset-block-start: -3px;
  inset-inline-end: 0;
  inline-size: 1px;
  block-size: calc(var(--rail-height) + 6px);
  background: var(--text-secondary);
}

.rail__reset {
  color: var(--text-secondary);
  font-size: 0.75rem;
  white-space: nowrap;
}

/* --- 状态处理 --- */

/* 旧快照：保留数值，但明确降级；终点标记换成警示色 */
.rail--faded .rail__fill {
  background: color-mix(in srgb, var(--text-primary) 38%, transparent);
}

.rail--faded .rail__value {
  color: var(--text-secondary);
}

.rail--tone-warning .rail__terminal {
  background: var(--status-warning);
  inline-size: 2px;
}

.rail--tone-critical .rail__terminal {
  background: var(--status-error);
  inline-size: 2px;
}

.rail--loading .rail__track {
  overflow: hidden;
}

.rail--loading .rail__track::after {
  content: "";
  position: absolute;
  inset: 0;
  background: linear-gradient(90deg, transparent, var(--rail-tick), transparent);
}

.rail--loading .rail__value,
.rail--empty .rail__value {
  color: var(--text-secondary);
}

@media (prefers-reduced-motion: no-preference) {
  .rail__fill {
    transition: inline-size var(--motion-base) var(--ease-out);
  }

  .rail--loading .rail__track::after {
    animation: rail-scan 1.5s var(--ease-out) infinite;
  }
}

@keyframes rail-scan {
  from {
    transform: translateX(-100%);
  }

  to {
    transform: translateX(100%);
  }
}
</style>
