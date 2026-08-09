<script setup lang="ts">
/**
 * Provider Lane —— 稳定的信息语法。
 *
 * 每条 lane 从身份读到剩余额度，再读到重置端点与新鲜度。失败时保持位置和既有数据，
 * 不切换成完全不同的错误卡片；无凭据时保持相同骨架，用说明替换额度区域。
 *
 * 层级靠阴影表达，不用左侧色条。百分比读数的颜色来自余量分档（ADR-0017），
 * 状态由总体状态点（Overall Signal）承担——两者是独立的两个维度，卡片内不再出现状态提示条。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import { useSettingsStore } from "../features/settings/store";
import {
  primaryWindow,
  secondaryWindows,
  type ProviderSnapshot,
  type QuotaWindow,
} from "../features/quota/contracts";
import type { ServiceStatus } from "../features/quota/serviceStatus";
import type { UsageProviderCosts } from "../features/usage/contracts";
import { formatAbsolute, formatPercent, splitPercent } from "../lib/format";
import { planLabel, providerLabel, windowCode, windowLabel } from "../lib/labels";
import { displayQuotaTone, type QuotaTone } from "../lib/quotaTone";
import { hasQuotaValues, presentProvider } from "../lib/status";
import { useTimeText } from "../lib/useTimeText";
import QuotaProgress from "./QuotaProgress.vue";
import UsageCostReadout from "./UsageCostReadout.vue";
const props = defineProps<{
  provider: ProviderSnapshot;
  variant: "compact" | "full";
  usageCosts?: UsageProviderCosts;
  usageScanning?: boolean;
  /** 官方服务状态（Statuspage 状态链，ADR-0026）。与额度状态无关。 */
  serviceStatus?: ServiceStatus | null;
}>();

const { t, locale } = useI18n();
const settingsStore = useSettingsStore();
const { reset, past } = useTimeText();
const presentation = computed(() => presentProvider(props.provider));
const name = computed(() => providerLabel(t, props.provider.provider));

/**
 * 官方服务状态圆点（ADR-0026）。`unknown` 或没有快照时不画点；
 * 开关只控制绘制，后台拉取不受影响。
 */
const serviceStatusShown = computed(
  () =>
    (settingsStore.settings?.showServiceStatus ?? true) &&
    props.serviceStatus !== undefined &&
    props.serviceStatus !== null &&
    props.serviceStatus.indicator !== "unknown",
);
const serviceStatusTone = computed(() => {
  const indicator = props.serviceStatus?.indicator;
  switch (indicator) {
    case "none":
      return "success";
    case "minor":
      return "warning";
    case "major":
      return "low";
    case "critical":
      return "error";
    case "maintenance":
      return "maintenance";
    default:
      return null;
  }
});
const serviceStatusLabel = computed(() => {
  const indicator = props.serviceStatus?.indicator;
  return indicator ? t(`status.service.${indicator}`) : "";
});
/** tooltip：description 优先，缺失时用 indicator 文案；有更新时刻时附「N 前更新」。 */
const serviceStatusTitle = computed(() => {
  const status = props.serviceStatus;
  if (!status) {
    return "";
  }
  const head = status.description?.trim() || serviceStatusLabel.value;
  const age = status.updatedAt ? past(status.updatedAt) : null;
  return age ? t("status.serviceUpdated", { head, age }) : head;
});

/**
 * 身份是一段并置的次要信息：账号在前、套餐在后。
 *
 * 宽度不足时收缩的是账号，不是套餐——套餐决定额度上限，属于要读的信息；
 * 账号只用来确认「是不是我这个号」，截断后仍然认得出来。
 */
const plan = computed(() => {
  const value = props.provider.identity?.plan;
  return value ? planLabel(value) : null;
});
/** 隐私模式：仅隐藏紧凑入口的账号标识，不承诺隐私隔离（cc-bar F-24 边界）。 */
const privacyMode = computed(() => settingsStore.settings?.privacyMode ?? false);
const account = computed(() =>
  privacyMode.value ? null : (props.provider.identity?.account ?? null),
);

const primary = computed(() => primaryWindow(props.provider.snapshot));

const secondaries = computed(() => secondaryWindows(props.provider.snapshot));

const showsRails = computed(
  () => hasQuotaValues(props.provider) || presentation.value.rail === "loading",
);

/**
 * 异常状态不在卡片内承载：popover 卡片只显示用量，状态信息由总体状态点
 * 的颜色与 tooltip 表达（见 OverallSignal.vue），完整说明不再占卡片空间。
 */

/* ---------- compact 分支（原型评审稿结构） ---------- */

const primaryCode = computed(() =>
  primary.value ? windowCode(props.provider.provider, primary.value) : "",
);
const primaryLabel = computed(() => (primary.value ? windowLabel(t, primary.value) : ""));
const primaryTone = computed(() =>
  displayQuotaTone(primary.value?.remainingPercent ?? null, presentation.value.rail),
);
const primaryHasValue = computed(
  () => primary.value?.remainingPercent !== null && presentation.value.rail !== "loading",
);
const primaryParts = computed(() => {
  const value = primary.value;
  return primaryHasValue.value && value
    ? splitPercent(locale.value, value.remainingPercent)
    : { value: t("quota.noValue"), unit: "" };
});
const primaryFill = computed(() => {
  const percent = primary.value?.remainingPercent;
  return primaryHasValue.value && percent !== undefined ? Math.max(0, Math.min(100, percent)) : 0;
});
const primaryReset = computed(() => reset(primary.value?.resetsAt ?? null));
const primaryA11y = computed(() =>
  primary.value
    ? t("a11y.quotaRail", { provider: name.value, window: primaryLabel.value })
    : name.value,
);
const primaryValueA11y = computed(() => {
  const value = primary.value;
  return primaryHasValue.value && value
    ? t("a11y.remaining", { percent: formatPercent(locale.value, value.remainingPercent) })
    : t("a11y.noQuota");
});
const primaryResetDescription = computed(() =>
  primary.value?.resetsAt
    ? t("quota.resetsAt", { time: formatAbsolute(locale.value, primary.value.resetsAt) })
    : t("quota.resetsUnknown"),
);

/* 次级窗口：单行读数 + 细进度条，tone 与主读数同一维度 */
function secondaryParts(window: QuotaWindow): { value: string; unit: string } {
  if (window.remainingPercent !== null && presentation.value.rail !== "loading") {
    return splitPercent(locale.value, window.remainingPercent);
  }
  return { value: t("quota.noValue"), unit: "" };
}
function secondaryHasValue(window: QuotaWindow): boolean {
  return window.remainingPercent !== null && presentation.value.rail !== "loading";
}
function secondaryFill(window: QuotaWindow): number {
  if (!secondaryHasValue(window)) {
    return 0;
  }
  return Math.max(0, Math.min(100, window.remainingPercent ?? 0));
}
function secondaryReset(window: QuotaWindow): string {
  return reset(window.resetsAt);
}
function secondaryResetDescription(window: QuotaWindow): string {
  return window.resetsAt
    ? t("quota.resetsAt", { time: formatAbsolute(locale.value, window.resetsAt) })
    : t("quota.resetsUnknown");
}
function secondaryCode(window: QuotaWindow): string {
  return windowCode(props.provider.provider, window);
}
function secondaryLabel(window: QuotaWindow): string {
  return windowLabel(t, window);
}
function secondaryTone(window: QuotaWindow): QuotaTone {
  return displayQuotaTone(window.remainingPercent, presentation.value.rail);
}
function secondaryA11y(window: QuotaWindow): string {
  return t("a11y.quotaRail", { provider: name.value, window: windowLabel(t, window) });
}
function secondaryValueA11y(window: QuotaWindow): string {
  return secondaryHasValue(window)
    ? t("a11y.remaining", { percent: formatPercent(locale.value, window.remainingPercent ?? 0) })
    : t("a11y.noQuota");
}
</script>

<template>
  <article class="lane" :class="[`lane--${variant}`, `lane--${presentation.tone}`]">
    <!-- compact：原型评审稿结构（身份行 + 读数行 + 独立进度条 + 费用行） -->
    <template v-if="variant === 'compact'">
      <header class="lane-id">
        <span class="lane-logo" :class="`lane-logo--${provider.provider}`" aria-hidden="true">
          <svg v-if="provider.provider === 'codex'" viewBox="0 0 100 100" fill="currentColor">
            <path
              d="M83.7733 42.8087C84.6678 40.1149 84.9771 37.2613 84.6807 34.4385C84.3843 31.6156 83.489 28.8885 82.0544 26.4394C77.6908 18.8436 68.9203 14.9365 60.3548 16.7725C57.9831 14.1344 54.9591 12.1668 51.5864 11.0673C48.2137 9.96772 44.611 9.77498 41.1402 10.5084C37.6694 11.2418 34.4527 12.8755 31.8132 15.2455C29.1736 17.6155 27.204 20.6383 26.1024 24.0103C23.3212 24.5806 20.6938 25.738 18.3958 27.405C16.0977 29.0721 14.1819 31.2104 12.7765 33.6772C8.36538 41.2609 9.3669 50.8267 15.2527 57.3327C14.3549 60.0251 14.0424 62.8782 14.3361 65.7012C14.6298 68.5241 15.523 71.2518 16.9558 73.7017C21.325 81.3002 30.1011 85.207 38.6712 83.3686C40.5554 85.4904 42.8707 87.1858 45.4623 88.3416C48.0539 89.4975 50.8622 90.0871 53.6999 90.0713C62.4793 90.079 70.2575 84.4114 72.9393 76.0515C75.7201 75.4802 78.347 74.3225 80.6449 72.6555C82.9427 70.9886 84.8587 68.8507 86.2649 66.3846C90.6227 58.8145 89.6172 49.3005 83.7733 42.8087ZM53.6999 84.8356C50.1955 84.8411 46.801 83.6129 44.1116 81.3661L44.5848 81.098L60.5123 71.9043C60.9087 71.6718 61.2379 71.3402 61.4674 70.942C61.6969 70.5439 61.8189 70.0929 61.8215 69.6333V47.1769L68.5553 51.072C68.6225 51.1063 68.6694 51.1707 68.6814 51.2456V69.854C68.6641 78.1208 61.9667 84.8183 53.6999 84.8356ZM21.4977 71.0843C19.7402 68.0497 19.1092 64.4925 19.7156 61.0386L20.1885 61.3225L36.1321 70.5165C36.5266 70.748 36.9757 70.87 37.4331 70.87C37.8905 70.87 38.3396 70.748 38.7341 70.5165L58.21 59.2883V67.0628C58.2081 67.1031 58.1973 67.1424 58.1782 67.1779C58.1591 67.2134 58.1322 67.2441 58.0996 67.2678L41.9671 76.5722C34.798 80.7022 25.6388 78.2463 21.4977 71.0843ZM17.3026 36.3898C19.0723 33.3357 21.8655 31.0062 25.1878 29.8138V48.7376C25.1818 49.1949 25.2986 49.6453 25.5261 50.042C25.7535 50.4387 26.0833 50.7671 26.4809 50.9928L45.8622 62.1739L39.1283 66.069C39.0919 66.0883 39.0513 66.0984 39.0101 66.0984C38.9689 66.0984 38.9283 66.0883 38.8919 66.069L22.7908 56.7809C15.6359 52.6337 13.1822 43.4816 17.3026 36.3112V36.3898ZM72.624 49.2426L53.1792 37.9512L59.8976 34.0718C59.9341 34.0524 59.9747 34.0423 60.016 34.0423C60.0573 34.0423 60.0979 34.0524 60.1344 34.0718L76.2355 43.3761C78.6973 44.7966 80.7043 46.8882 82.0221 49.4065C83.3398 51.9249 83.914 54.7661 83.6775 57.5985C83.4411 60.431 82.4038 63.1377 80.6867 65.4027C78.9696 67.6677 76.6436 69.3975 73.9803 70.3901V51.466C73.9663 51.0096 73.834 50.5647 73.5962 50.1749C73.3584 49.7851 73.0234 49.4638 72.624 49.2426ZM79.3261 39.1657L78.8529 38.8815L62.9411 29.6089C62.5442 29.376 62.0924 29.2532 61.6322 29.2532C61.172 29.2532 60.7202 29.376 60.3233 29.6089L40.8629 40.8374V33.0628C40.8587 33.0233 40.8654 32.9834 40.882 32.9473C40.8987 32.9113 40.9248 32.8803 40.9575 32.8579L57.0586 23.5692C59.5263 22.1476 62.3478 21.458 65.193 21.5811C68.0382 21.7042 70.7896 22.6348 73.1253 24.2642C75.461 25.8936 77.2845 28.1543 78.3825 30.782C79.4806 33.4097 79.8077 36.2957 79.3257 39.1025V39.1657H79.3261ZM37.1888 52.9484L30.455 49.069C30.4213 49.0487 30.3925 49.0212 30.3707 48.9884C30.3488 48.9557 30.3345 48.9186 30.3286 48.8797V30.3188C30.3323 27.4714 31.1466 24.6839 32.6761 22.2822C34.2057 19.8805 36.3874 17.9639 38.9661 16.7564C41.5448 15.549 44.4139 15.1005 47.2381 15.4636C50.0622 15.8267 52.7247 16.9862 54.9141 18.8067L54.4409 19.0748L38.5134 28.2686C38.117 28.5011 37.7879 28.8327 37.5584 29.2308C37.329 29.629 37.207 30.0799 37.2045 30.5395L37.1888 52.9487V52.9484ZM40.8472 45.0632L49.5209 40.0643L58.21 45.0635V55.0615L49.5523 60.0608L40.8632 55.0615L40.8472 45.0632Z"
            />
          </svg>
          <svg v-else viewBox="0 0 100 100" fill="currentColor">
            <path
              d="M25.7146 63.2153L41.4393 54.3917L41.7025 53.6226L41.4393 53.1976H40.6705L38.0394 53.0359L29.054 52.7929L21.2624 52.4691L13.7134 52.0644L11.8111 51.6594L10.0303 49.3118L10.2123 48.138L11.8111 47.0657L14.0981 47.2681L19.1574 47.6119L26.7467 48.138L32.2516 48.4618L40.4073 49.3118H41.7025L41.8846 48.7857L41.4393 48.4618L41.0955 48.138L33.243 42.8155L24.7432 37.1894L20.2909 33.9513L17.8824 32.3119L16.6684 30.774L16.1422 27.4147L18.328 25.0062L21.2624 25.2088L22.0112 25.4112L24.9861 27.6979L31.3407 32.616L39.6381 38.7273L40.8525 39.7391L41.3381 39.395L41.399 39.1523L40.8525 38.2415L36.3394 30.0858L31.5227 21.7883L29.3775 18.3478L28.811 16.2837C28.6087 15.4334 28.4669 14.7252 28.4669 13.8549L30.9563 10.4753L32.3321 10.0303L35.6515 10.4756L37.0479 11.6897L39.112 16.4052L42.4513 23.8327L47.6321 33.9313L49.15 36.9265L49.9594 39.6991L50.2632 40.5491H50.7894V40.0632L51.2141 34.3766L52.0035 27.3944L52.7726 18.4087L53.0358 15.8793L54.2905 12.8435L56.7795 11.2041L58.7224 12.135L60.3212 14.422L60.0986 15.899L59.1474 22.0718L57.2857 31.7458L56.0713 38.2218H56.7795L57.5892 37.4121L60.8677 33.061L66.3723 26.18L68.801 23.448L71.6342 20.4325L73.4556 18.9957H76.8962L79.4255 22.7601L78.2926 26.6456L74.7509 31.1384L71.8163 34.943L67.607 40.6097L64.9758 45.1431L65.2188 45.5072L65.8464 45.4466L75.358 43.4228L80.4984 42.4917L86.6304 41.4393L89.4033 42.7346L89.7065 44.0502L88.6135 46.7419L82.0566 48.3607L74.3662 49.8989L62.9118 52.6109L62.77 52.7121L62.9321 52.9144L68.0925 53.4L70.2987 53.5214H75.7021L85.7601 54.2702L88.3912 56.0108L89.9697 58.1358L89.7065 59.7545L85.6589 61.8189L80.1949 60.5236L67.4452 57.4881L63.0735 56.3952H62.4665V56.7596L66.1093 60.3213L72.7877 66.3523L81.1461 74.1236L81.5707 76.0462L80.4984 77.5638L79.3649 77.4021L72.0186 71.8772L69.1854 69.3879L62.77 63.9844H62.3453V64.5509L63.8223 66.7164L71.6342 78.4544L72.0389 82.0567L71.4725 83.2308L69.4487 83.939L67.2222 83.534L62.6485 77.1189L57.9333 69.8937L54.1284 63.4177L53.6631 63.6809L51.4167 87.8651L50.3644 89.0995L47.9356 90.0303L45.9121 88.4924L44.8392 86.0031L45.9118 81.0852L47.2071 74.6701L48.2594 69.5699L49.2106 63.2356L49.7773 61.131L49.7367 60.9892L49.2715 61.0498L44.4954 67.607L37.23 77.4224L31.4825 83.5746L30.1063 84.1211L27.7181 82.8864L27.9408 80.6805L29.2763 78.7177L37.2297 68.5988L42.026 62.3248L45.1227 58.7025L45.1024 58.176H44.9204L23.7917 71.8975L20.0274 72.3831L18.4083 70.8655L18.6106 68.3761L19.3798 67.5664L25.7343 63.195L25.7146 63.2153Z"
            />
          </svg>
        </span>
        <h3 class="lane-name" translate="no">{{ name }}</h3>
        <span v-if="plan" class="lane-chip" translate="no">{{ plan }}</span>
        <p v-if="account" class="lane-account" :title="account">{{ account }}</p>
        <span
          v-if="serviceStatusShown && serviceStatusTone"
          class="lane-status"
          :class="`lane-status--${serviceStatusTone}`"
          role="img"
          :aria-label="serviceStatusLabel"
          :title="serviceStatusTitle"
        />
      </header>

      <template v-if="showsRails">
        <div class="lane-reading">
          <span class="lane-percent numeric" :class="`lane-percent--${primaryTone}`">
            <span>{{ primaryParts.value }}</span>
            <small v-if="primaryParts.unit">{{ primaryParts.unit }}</small>
          </span>
          <span v-if="primaryCode" class="lane-window" :title="primaryLabel" aria-hidden="true">{{
            primaryCode
          }}</span>
          <span class="lane-reset numeric">
            <span
              class="lane-reset__time"
              :title="primaryResetDescription"
              :aria-label="primaryResetDescription"
              >{{ primaryReset }}</span
            >
            <span class="lane-reset__label" aria-hidden="true">{{ t("quota.resetLabel") }}</span>
          </span>
        </div>

        <div
          class="lane-bar"
          :class="[`lane-bar--${primaryTone}`, `lane-bar--${presentation.rail}`]"
          role="progressbar"
          :aria-label="primaryA11y"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-valuenow="primaryHasValue ? primaryFill : undefined"
          :aria-valuetext="primaryValueA11y"
        >
          <span
            v-if="primaryHasValue"
            class="lane-bar__fill"
            :style="{ inlineSize: `${primaryFill}%` }"
          />
        </div>

        <div v-if="secondaries.length > 0" class="lane__secondaries lane__secondaries--compact">
          <div v-for="window in secondaries" :key="window.id" class="lane-secondary">
            <div class="lane-reading">
              <span
                class="lane-percent lane-percent--secondary numeric"
                :class="`lane-percent--${secondaryTone(window)}`"
              >
                <span>{{ secondaryParts(window).value }}</span>
                <small v-if="secondaryParts(window).unit">{{ secondaryParts(window).unit }}</small>
              </span>
              <span class="lane-window" :title="secondaryLabel(window)" aria-hidden="true">{{
                secondaryCode(window)
              }}</span>
              <span class="lane-reset numeric">
                <span
                  class="lane-reset__time"
                  :title="secondaryResetDescription(window)"
                  :aria-label="secondaryResetDescription(window)"
                  >{{ secondaryReset(window) }}</span
                >
                <span class="lane-reset__label" aria-hidden="true">{{
                  t("quota.resetLabel")
                }}</span>
              </span>
            </div>
            <div
              class="lane-bar lane-bar--secondary"
              :class="[`lane-bar--${secondaryTone(window)}`, `lane-bar--${presentation.rail}`]"
              role="progressbar"
              :aria-label="secondaryA11y(window)"
              aria-valuemin="0"
              aria-valuemax="100"
              :aria-valuenow="secondaryHasValue(window) ? secondaryFill(window) : undefined"
              :aria-valuetext="secondaryValueA11y(window)"
            >
              <span
                v-if="secondaryHasValue(window)"
                class="lane-bar__fill"
                :style="{ inlineSize: `${secondaryFill(window)}%` }"
              />
            </div>
          </div>
        </div>
      </template>

      <!-- 没有额度可显示时保持同一骨架：读数位置留占位符，不伪造 0% -->
      <div v-else class="lane-reading lane-reading--blank">
        <span class="lane-percent lane-percent--none numeric">{{ t("quota.noValue") }}</span>
      </div>

      <UsageCostReadout
        v-if="usageCosts"
        :provider-name="name"
        :costs="usageCosts"
        :scanning="usageScanning"
      />
    </template>

    <!-- full：主窗口用量页形态 -->
    <template v-else>
      <header class="lane__header">
        <h3 class="lane__name" translate="no">{{ name }}</h3>

        <p v-if="account || plan" class="lane__identity">
          <span v-if="account" class="lane__account" :title="account">{{ account }}</span>
          <span v-if="plan" class="lane__plan" translate="no">{{ plan }}</span>
        </p>
      </header>

      <template v-if="showsRails">
        <QuotaProgress
          :label="primary ? windowLabel(t, primary) : ''"
          :code="primary ? windowCode(provider.provider, primary) : ''"
          :remaining-percent="primary?.remainingPercent ?? null"
          :resets-at="primary?.resetsAt ?? null"
          :treatment="presentation.rail"
          :a11y-label="
            primary
              ? t('a11y.quotaRail', { provider: name, window: windowLabel(t, primary) })
              : name
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
            :code="windowCode(provider.provider, window)"
            :remaining-percent="window.remainingPercent"
            :resets-at="window.resetsAt"
            :treatment="presentation.rail"
            :a11y-label="t('a11y.quotaRail', { provider: name, window: windowLabel(t, window) })"
            :size="variant"
          />
        </div>
      </template>

      <!-- 没有额度可显示时保持同一骨架：读数位置留占位符，不伪造 0% -->
      <div v-else class="lane__blank-row">
        <p class="lane__blank numeric" :class="`lane__blank--${variant}`">
          {{ t("quota.noValue") }}
        </p>
      </div>
    </template>
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
  flex: 0 0 auto;
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  letter-spacing: -0.008em;
  white-space: nowrap;
}

.lane--full .lane__name {
  font-size: 1.0625rem;
  font-weight: 650;
  letter-spacing: -0.014em;
}

/*
 * 身份紧跟在 Provider 名之后，吃掉整行剩余宽度——不设百分比上限：380px 下的 42%
 * 只有约 140px，常见邮箱刚好卡在临界，会无理由地省略。
 */
.lane__identity {
  display: flex;
  align-items: baseline;
  gap: var(--space-1);
  min-inline-size: 0;
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.lane--compact .lane__identity {
  opacity: 0.75;
}

.lane--full .lane__identity {
  font-size: 0.8125rem;
}

/* 只有账号会被压缩，套餐永远完整 */
.lane__account {
  min-inline-size: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.lane__plan {
  flex: 0 0 auto;
  white-space: nowrap;
}

/* 分隔符由样式提供：`·` 语言中立，不需要为它拼接文案 */
.lane__account + .lane__plan::before {
  content: "·";
  margin-inline-end: var(--space-1);
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

/* 与大读数同体量：无额度时骨架不塌陷，位置也不跳 */
.lane__blank {
  margin: 0;
  color: var(--text-secondary);
  font-size: 2rem;
  font-weight: 650;
  line-height: 1;
  letter-spacing: -0.035em;
}

.lane__blank-row {
  display: flex;
  align-items: end;
  justify-content: space-between;
  gap: var(--space-4);
  min-inline-size: 0;
}

.lane__blank--full {
  font-size: 2.5rem;
  letter-spacing: -0.04em;
}

/* ============================================================
   compact（原型评审稿第 1 节）
   ============================================================ */

.lane--compact {
  display: flex;
  flex-direction: column;
  gap: 0;
  padding: 13px 16px 14px;
  border: 1px solid color-mix(in srgb, var(--border-subtle) 80%, transparent);
  border-radius: 0.875rem;
}

.lane--compact > .lane-id {
  margin-block-end: 10px;
}

.lane--compact > .lane-reading {
  margin-block-end: 7px;
}

.lane--compact > .lane__secondaries--compact {
  margin-block-start: 11px;
}

.lane--compact > .usage-cost {
  margin-block-start: 10px;
}

/* ---------- 身份行 ---------- */

.lane-id {
  display: flex;
  align-items: center;
  gap: 8px;
  min-inline-size: 0;
}

/* 服务 logo tile：22px squircle + 14px logo（参考 cc-bar ServiceTile） */
.lane-logo {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
  inline-size: 22px;
  block-size: 22px;
  border-radius: 6px;
}

.lane-logo svg {
  display: block;
  inline-size: 14px;
  block-size: 14px;
}

/* Codex 走 OpenAI 官方观感：白底黑 logo + 极细边框（深浅色一致） */
.lane-logo--codex {
  background: #ffffff;
  border: 0.5px solid rgb(0 0 0 / 12%);
  color: #000000;
}

/* Claude：品牌色底 + 白 logo */
.lane-logo--claude {
  background: var(--cat-claude);
  color: #ffffff;
}

.lane-name {
  flex: 0 0 auto;
  margin: 0;
  font-size: 0.875rem;
  font-weight: 650;
  letter-spacing: -0.01em;
  white-space: nowrap;
}

.lane-chip {
  flex: 0 0 auto;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--track-background);
  color: var(--text-secondary);
  font-size: 0.65625rem;
  font-weight: 620;
  line-height: 1.5;
  white-space: nowrap;
}

.lane-account {
  margin: 0;
  min-inline-size: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
  font-size: 0.71875rem;
}

/* 官方服务状态点：靠右，与 cc-bar headerRow 一致（ADR-0026）。unknown 不绘制。 */
.lane-status {
  margin-inline-start: auto;
  flex: none;
  inline-size: 6px;
  block-size: 6px;
  border-radius: 50%;
  background: var(--status-success);
}

.lane-status--warning {
  background: var(--status-warning);
}

.lane-status--low {
  background: var(--status-low);
}

.lane-status--error {
  background: var(--status-error);
}

.lane-status--maintenance {
  background: var(--status-maintenance);
}

/* ---------- 读数行：% 与重置时间同一行（One Reading Path） ---------- */

.lane-reading {
  display: flex;
  align-items: baseline;
  gap: 7px;
  min-inline-size: 0;
}

.lane-reading--blank {
  min-block-size: 1.625rem;
  align-items: center;
}

/* 大读数 26px（原型 🚩：44px → 26px，压制戏剧感） */
.lane-percent {
  display: inline-flex;
  align-items: baseline;
  font-size: 1.625rem;
  font-weight: 700;
  letter-spacing: -0.03em;
  line-height: 1;
}

.lane-percent small {
  font-size: 0.9375rem;
  font-weight: 650;
  letter-spacing: 0;
}

.lane-percent--warning {
  color: var(--status-warning);
}

.lane-percent--low {
  color: var(--status-low);
}

.lane-percent--danger {
  color: var(--status-error);
}

.lane-percent--none {
  color: var(--text-secondary);
}

/* ADR-0019 定宽短码，只调排版位置 */
.lane-window {
  color: var(--text-secondary);
  font-family: var(--font-data);
  font-size: 0.625rem;
  font-weight: 650;
  letter-spacing: 0.07em;
  white-space: nowrap;
}

.lane-reset {
  margin-inline-start: auto;
  display: inline-flex;
  align-items: baseline;
  gap: 5px;
  font-size: 0.78125rem;
  font-weight: 570;
  white-space: nowrap;
}

.lane-reset__label {
  color: var(--text-secondary);
  font-size: 0.65625rem;
  font-weight: 500;
}

/* ---------- 进度条：6px 圆条。ok 档 success 绿（原型 🚩，推翻 ADR-0017 取值） ---------- */

.lane-bar {
  position: relative;
  block-size: 6px;
  border-radius: 999px;
  background: var(--track-background);
  overflow: hidden;
}

.lane-bar__fill {
  position: absolute;
  inset-block: 0;
  inset-inline-start: 0;
  border-radius: inherit;
  background: var(--status-success);
}

.lane-bar--warning .lane-bar__fill {
  background: var(--status-warning);
}

.lane-bar--low .lane-bar__fill {
  background: var(--status-low);
}

.lane-bar--danger .lane-bar__fill {
  background: var(--status-error);
}

.lane-bar--faded .lane-bar__fill {
  background: color-mix(in srgb, var(--text-secondary) 45%, transparent);
}

.lane-bar--secondary {
  block-size: 4px;
}

.lane-bar--loading::after {
  content: "";
  position: absolute;
  inset: 0;
  background: linear-gradient(90deg, transparent, var(--border-subtle), transparent);
}

/* ---------- 次级窗口：同族样式、更细的条 ---------- */

.lane__secondaries--compact {
  display: grid;
  gap: 11px;
}

.lane__secondaries--compact .lane-secondary {
  margin: 0;
  padding-block-start: 11px;
  border-block-start: 1px dashed color-mix(in srgb, var(--border-subtle) 90%, transparent);
}

.lane__secondaries--compact .lane-reading {
  margin-block-end: 6px;
}

.lane-percent--secondary {
  font-size: 0.9375rem;
  font-weight: 680;
  letter-spacing: -0.01em;
}

.lane-percent--secondary small {
  font-size: 0.6875rem;
}

@media (prefers-reduced-motion: no-preference) {
  .lane-bar__fill {
    transition: inline-size var(--motion-base) var(--ease-out);
  }

  .lane-bar--loading::after {
    animation: lane-bar-scan 1.5s var(--ease-out) infinite;
  }
}

@keyframes lane-bar-scan {
  from {
    transform: translateX(-100%);
  }

  to {
    transform: translateX(100%);
  }
}
</style>
