<script setup lang="ts">
/**
 * Quota Progress —— CC Trace 的签名元素。
 *
 * 两种排布，见 `docs/设计方向与状态规范.md` 第 7.2 节：
 * - `primary`：左列是大百分比与窗口短码，右列是全宽进度条、重置倒计时与辅助读数。
 * - `secondary`：单行（短码 + 细进度条 + 百分比 + 倒计时），进度条吸收剩余宽度。
 *
 * 读数一律定宽（ADR-0019）：短码是语言中立的大写拉丁短码，重置时间是紧凑倒计时
 * （`6d2h`）。会随日期变宽的绝对时钟放不进 380px 面板，完整窗口名与绝对时刻改由
 * `title` 与无障碍名称承担，因此短码不是这两个信息的唯一载体。
 *
 * 颜色由**余量基调**驱动（`lib/quotaTone.ts`），不是状态基调：状态基调负责提示条与
 * 状态词，余量基调负责这里的读数与填充，见 ADR-0017。快照不新鲜时自动降级为中性。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import { formatAbsolute, formatPercent, splitPercent } from "../lib/format";
import { displayQuotaTone } from "../lib/quotaTone";
import type { RailTreatment } from "../lib/status";
import { useTimeText } from "../lib/useTimeText";

const props = withDefaults(
  defineProps<{
    /** 已本地化的完整窗口名。只进 `title` 与无障碍名称，不出现在读数行。 */
    label: string;
    /** 读数行上显示的窗口短码，如 `5HOUR`、`ALL`。 */
    code: string;
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
const { reset } = useTimeText();

const hasValue = computed(() => props.remainingPercent !== null && props.treatment !== "loading");

const tone = computed(() => displayQuotaTone(props.remainingPercent, props.treatment));

/** 单行读数用合成形态；大读数用拆开的形态给数字和 `%` 分设字号。 */
const valueText = computed(() =>
  hasValue.value && props.remainingPercent !== null
    ? formatPercent(locale.value, props.remainingPercent)
    : t("quota.noValue"),
);

const valueParts = computed(() =>
  hasValue.value && props.remainingPercent !== null
    ? splitPercent(locale.value, props.remainingPercent)
    : { value: t("quota.noValue"), unit: "" },
);

/** 进度条填充比例。没有数值时为 0，且不渲染填充块。 */
const fillPercent = computed(() =>
  hasValue.value && props.remainingPercent !== null
    ? Math.max(0, Math.min(100, props.remainingPercent))
    : 0,
);

/** 定宽倒计时。缺失时给占位符，旁边的「重置」标签仍在，语义不丢。 */
const resetText = computed(() => reset(props.resetsAt));

/**
 * 紧凑倒计时对屏幕阅读器没有意义，也不满足「相对时间必须同时提供绝对值」。
 * 两者都由这条完整说法承担：`title` 给鼠标，`aria-label` 给辅助技术。
 */
const resetDescription = computed(() =>
  props.resetsAt
    ? t("quota.resetsAt", { time: formatAbsolute(locale.value, props.resetsAt) })
    : t("quota.resetsUnknown"),
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
    <!-- primary：左列读数与短码，右列进度条与倒计时 -->
    <template v-if="emphasis === 'primary'">
      <p class="progress__headline">
        <span class="progress__value numeric">
          <span class="progress__number">{{ valueParts.value }}</span>
          <span v-if="valueParts.unit" class="progress__unit">{{ valueParts.unit }}</span>
        </span>
        <!-- 短码是完整窗口名的冗余视觉形态：辅助技术从进度条名称拿到完整名 -->
        <span v-if="code" class="progress__code" :title="label" aria-hidden="true">{{ code }}</span>
      </p>

      <div class="progress__meter">
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

        <div class="progress__readout">
          <p class="progress__reset-group">
            <span
              class="progress__reset numeric"
              :title="resetDescription"
              :aria-label="resetDescription"
              >{{ resetText }}</span
            >
            <span class="progress__reset-label" aria-hidden="true">{{
              t("quota.resetLabel")
            }}</span>
          </p>

          <!-- Popover 在这里并列今日／本周费用；主窗口不传 slot，现有布局保持不变。 -->
          <slot name="aside" />
        </div>
      </div>
    </template>

    <!-- secondary：单行，短码占固定列让多行左边缘对齐，轨道吸收剩余宽度 -->
    <div v-else class="progress__row">
      <span class="progress__code" :title="label" aria-hidden="true">{{ code }}</span>

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
      <span
        class="progress__reset numeric"
        :title="resetDescription"
        :aria-label="resetDescription"
        >{{ resetText }}</span
      >
    </div>
  </div>
</template>

<style scoped>
/* 读数列按内容取宽，进度条列吃掉剩下的；minmax 里的 0 是让它真的可以收缩 */
.progress--primary {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: var(--space-4);
  align-items: center;
}

.progress__headline {
  display: grid;
  justify-items: center;
  gap: var(--space-1);
  margin: 0;
}

/* 数字与 % 按基线对齐：% 更小，顶对齐会让它浮在半空 */
.progress__value {
  display: flex;
  align-items: baseline;
}

.progress--primary .progress__number {
  font-size: 2rem;
  font-weight: 600;
  line-height: 1;
  letter-spacing: -0.035em;
}

.progress--primary.progress--full .progress__number {
  font-size: 2.5rem;
  font-weight: 650;
  letter-spacing: -0.04em;
}

/* % 是单位不是读数，压到一半字号并让它退后半档 */
.progress--primary .progress__unit {
  font-size: 1rem;
  opacity: 0.75;
}

.progress--primary.progress--full .progress__unit {
  font-size: 1.25rem;
}

.progress--secondary .progress__value {
  flex: 0 0 auto;
  font-size: 0.65625rem;
  font-weight: 500;
}

.progress--secondary.progress--full .progress__value {
  font-size: 0.8125rem;
  font-weight: 650;
}

/*
 * 窗口短码。字距是给大写拉丁字母补的，短码永远是拉丁大写，因此不随界面语言归零
 * ——这一点与 `.utility-label` 的 `:lang(zh)` 例外不同。
 */
.progress__code {
  color: var(--text-secondary);
  font-size: 0.5625rem;
  font-weight: 600;
  letter-spacing: 0.06em;
  line-height: 1;
  opacity: 0.7;
  white-space: nowrap;
}

.progress--full .progress__code {
  font-size: 0.6875rem;
  font-weight: 650;
  opacity: 1;
}

/* 固定列让多条次级额度的短码、轨道起点、读数全部竖向对齐 */
.progress--secondary .progress__code {
  flex: 0 0 3.5rem;
  overflow: hidden;
  text-overflow: ellipsis;
}

.progress__meter {
  display: grid;
  gap: var(--space-2);
}

/* 重置读数在左，Popover 的今日／本周费用可在右侧并列；没有 slot 时仍保持原位置 */
.progress__readout {
  display: flex;
  align-items: start;
  justify-content: space-between;
  gap: var(--space-2);
  margin: 0;
}

.progress__reset-group {
  display: grid;
  flex: 0 0 auto;
  justify-items: start;
  gap: 1px;
  margin: 0;
}

.progress__reset {
  color: var(--text-primary);
  font-size: 0.6875rem;
  font-weight: 500;
  line-height: 1;
  white-space: nowrap;
}

.progress--full .progress__reset {
  font-size: 0.8125rem;
  line-height: 1.1;
}

/* 行尾读数右对齐并留出最长倒计时的宽度，`43m` 与 `4h37m` 之间不推动轨道 */
.progress--secondary .progress__reset {
  flex: 0 0 auto;
  min-inline-size: 2.75rem;
  color: var(--text-secondary);
  font-size: 0.65625rem;
  font-weight: 400;
  opacity: 0.7;
  text-align: end;
}

.progress--secondary.progress--full .progress__reset {
  font-size: 0.6875rem;
  opacity: 1;
}

.progress__reset-label {
  color: var(--text-secondary);
  font-size: 0.59375rem;
  line-height: 1;
  opacity: 0.7;
}

.progress--full .progress__reset-label {
  font-size: 0.6875rem;
  line-height: 1.2;
  opacity: 1;
}

.progress__row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

/* 圆角进度条：层级靠体量表达，不是直轨道 */
.progress__track {
  position: relative;
  block-size: 0.4375rem;
  background: var(--track-background);
  border-radius: 999px;
  overflow: hidden;
}

.progress--secondary .progress__track {
  flex: 1 1 auto;
  min-inline-size: 1.75rem;
  block-size: 0.15625rem;
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
