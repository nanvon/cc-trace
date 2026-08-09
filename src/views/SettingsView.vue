<script setup lang="ts">
/**
 * 主窗口设置视图：只负责用户可以安全改变的应用偏好。
 *
 * 不承载额度详情、账号操作或 Provider 登录，见 `docs/信息架构与核心流程.md` 第 7 节。
 * 保存成功立即生效；写入失败时**保留原值**并明确提示。
 * 导航由侧边栏承担（ADR-0024）：本视图不再提供「返回用量」按钮。
 */
import { computed, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import { getUsageScanStatus, rebuildUsageData, refreshPricingCatalog } from "../features/usage/api";
import {
  APPEARANCE_OPTIONS,
  LANGUAGE_OPTIONS,
  REFRESH_INTERVAL_OPTIONS,
  type AppearancePreference,
  type LanguagePreference,
  type RefreshIntervalOption,
  type SettingsUpdate,
  type StatsServiceSource,
  type UsageServiceVisibility,
} from "../features/settings/contracts";
import { useSettingsStore } from "../features/settings/store";

const STATS_SERVICE_SOURCES: readonly StatsServiceSource[] = [
  "codex",
  "claude",
  "pi",
  "opencode",
] as const;

const { t } = useI18n();
const settings = useSettingsStore();
const pricingRefreshState = ref<"idle" | "success" | "partial" | "failure">("idle");
const pricingRefreshPending = ref(false);
const PRICING_REFRESH_STATE = {
  complete: "success",
  partial: "partial",
  failed: "failure",
} as const;

/** 数据重建：idle → confirm（二次确认防误触）→ running → success/failure。 */
const rebuildState = ref<"idle" | "confirm" | "running" | "success" | "failure">("idle");
const rebuildPending = ref(false);
let rebuildConfirmTimer: ReturnType<typeof setTimeout> | null = null;
let rebuildPoll: ReturnType<typeof setInterval> | null = null;

const rebuildLabel = computed(() => {
  if (rebuildState.value === "confirm") return t("settings.rebuildConfirm");
  if (rebuildPending.value) return t("settings.rebuildRunning");
  return t("settings.rebuildUsage");
});

const rebuildStatusText = computed(() => {
  switch (rebuildState.value) {
    case "running":
      return t("settings.rebuildRunningHint");
    case "success":
      return t("settings.rebuildSuccess");
    case "failure":
      return t("settings.rebuildFailure");
    default:
      return "";
  }
});

function scheduleRebuildConfirmReset(): void {
  if (rebuildConfirmTimer) clearTimeout(rebuildConfirmTimer);
  rebuildConfirmTimer = setTimeout(() => {
    if (rebuildState.value === "confirm") rebuildState.value = "idle";
  }, 10_000);
}

function clearRebuildPoll(): void {
  if (rebuildPoll) {
    clearInterval(rebuildPoll);
    rebuildPoll = null;
  }
}

/**
 * 首次点击进入确认态（10 秒未确认自动退回）；确认后删除本地统计并全量重扫，
 * 期间轮询扫描状态，结束后给出完成反馈。扫描中触发返回 busy，直接报失败文案。
 */
async function requestRebuild(): Promise<void> {
  if (rebuildPending.value) return;
  if (rebuildState.value !== "confirm") {
    rebuildState.value = "confirm";
    scheduleRebuildConfirmReset();
    return;
  }
  if (rebuildConfirmTimer) {
    clearTimeout(rebuildConfirmTimer);
    rebuildConfirmTimer = null;
  }
  rebuildPending.value = true;
  rebuildState.value = "running";
  try {
    await rebuildUsageData();
    rebuildPoll = setInterval(async () => {
      try {
        const status = await getUsageScanStatus();
        if (status.state === "running" || status.state === "cancelling") return;
        clearRebuildPoll();
        rebuildState.value = "success";
        rebuildPending.value = false;
      } catch {
        clearRebuildPoll();
        rebuildState.value = "failure";
        rebuildPending.value = false;
      }
    }, 1_000);
  } catch {
    rebuildState.value = "failure";
    rebuildPending.value = false;
  }
}

onUnmounted(() => {
  clearRebuildPoll();
  if (rebuildConfirmTimer) clearTimeout(rebuildConfirmTimer);
});

const current = computed(() => settings.settings);

const INTERVAL_LABEL: Record<RefreshIntervalOption, string> = {
  "1m": "settings.intervalOption.m1",
  "2m": "settings.intervalOption.m2",
  "3m": "settings.intervalOption.m3",
  "5m": "settings.intervalOption.m5",
  "10m": "settings.intervalOption.m10",
};

const LANGUAGE_LABEL: Record<LanguagePreference, string> = {
  system: "settings.languageOption.system",
  "zh-CN": "settings.languageOption.chinese",
  en: "settings.languageOption.english",
};

const APPEARANCE_LABEL: Record<AppearancePreference, string> = {
  system: "settings.appearanceOption.system",
  light: "settings.appearanceOption.light",
  dark: "settings.appearanceOption.dark",
};

/**
 * 写入失败时 store 保持原值，这里把控件也拉回去——否则用户会以为已经改成功了。
 */
async function commitSelect(
  key: "refreshInterval" | "language" | "appearance",
  value: string,
): Promise<void> {
  await settings.update({ [key]: value } as SettingsUpdate);
}

async function commitToggle(
  key: "launchAtLogin" | "privacyMode" | "showServiceStatus",
  checked: boolean,
): Promise<void> {
  await settings.update({ [key]: checked });
  if (!current.value) return;
  // 写入失败时 store 保持原值，控件由 :checked 绑定自然回到原值。
}

async function commitStatsService(source: StatsServiceSource, checked: boolean): Promise<void> {
  const visibility: UsageServiceVisibility = {
    codex: true,
    claude: true,
    pi: true,
    opencode: true,
    ...(current.value?.usageServiceVisibility ?? {}),
  };
  visibility[source] = checked;
  await settings.update({ usageServiceVisibility: visibility });
}

async function updatePricingCatalog(): Promise<void> {
  if (pricingRefreshPending.value) return;
  pricingRefreshPending.value = true;
  pricingRefreshState.value = "idle";
  try {
    const result = await refreshPricingCatalog();
    pricingRefreshState.value = PRICING_REFRESH_STATE[result];
  } catch {
    pricingRefreshState.value = "failure";
  } finally {
    pricingRefreshPending.value = false;
  }
}
</script>

<template>
  <main class="settings" :aria-label="t('a11y.settingsRegion')">
    <div class="settings__inner">
      <header class="settings__header">
        <h1 id="main-settings-title" tabindex="-1">{{ t("settings.title") }}</h1>
      </header>

      <template v-if="current">
        <p v-if="settings.writeFailed" class="settings__error" role="alert">
          <strong>{{ t("error.settingsWriteFailed.title") }}</strong>
          <span>{{ t("error.settingsWriteFailed.nextStep") }}</span>
        </p>

        <section class="sw-group">
          <h2>{{ t("settings.general") }}</h2>
          <div class="card sw-card">
            <div class="sw-row">
              <span class="sw-label">{{ t("settings.refreshInterval") }}</span>
              <select
                name="refresh-interval"
                :value="current.refreshInterval"
                autocomplete="off"
                @change="
                  commitSelect('refreshInterval', ($event.target as HTMLSelectElement).value)
                "
              >
                <option v-for="option in REFRESH_INTERVAL_OPTIONS" :key="option" :value="option">
                  {{ t(INTERVAL_LABEL[option]) }}
                </option>
              </select>
            </div>

            <div class="sw-row">
              <span class="sw-label">{{ t("settings.launchAtLogin") }}</span>
              <button
                type="button"
                class="toggle"
                :class="{ off: !current.launchAtLogin }"
                role="switch"
                :aria-checked="current.launchAtLogin"
                @click="commitToggle('launchAtLogin', !current.launchAtLogin)"
              >
                <span class="visually-hidden">{{ t("settings.launchAtLogin") }}</span>
              </button>
            </div>

            <div class="sw-row">
              <span class="sw-label">
                {{ t("settings.privacyMode") }}
                <span class="sw-desc">{{ t("settings.privacyModeDescription") }}</span>
              </span>
              <button
                type="button"
                class="toggle"
                :class="{ off: !current.privacyMode }"
                role="switch"
                :aria-checked="current.privacyMode"
                @click="commitToggle('privacyMode', !current.privacyMode)"
              >
                <span class="visually-hidden">{{ t("settings.privacyMode") }}</span>
              </button>
            </div>

            <div class="sw-row">
              <span class="sw-label">
                {{ t("settings.serviceStatus") }}
                <span class="sw-desc">{{ t("settings.serviceStatusDescription") }}</span>
              </span>
              <button
                type="button"
                class="toggle"
                :class="{ off: !current.showServiceStatus }"
                role="switch"
                :aria-checked="current.showServiceStatus"
                @click="commitToggle('showServiceStatus', !current.showServiceStatus)"
              >
                <span class="visually-hidden">{{ t("settings.serviceStatus") }}</span>
              </button>
            </div>
          </div>
        </section>

        <section class="sw-group">
          <h2>{{ t("settings.appearanceAndLanguage") }}</h2>
          <div class="card sw-card">
            <div class="sw-row">
              <span class="sw-label">{{ t("settings.language") }}</span>
              <div class="segmented" role="group" :aria-label="t('settings.language')">
                <button
                  v-for="option in LANGUAGE_OPTIONS"
                  :key="option"
                  type="button"
                  :class="{ on: current.language === option }"
                  :aria-pressed="current.language === option"
                  @click="commitSelect('language', option)"
                >
                  {{ t(LANGUAGE_LABEL[option]) }}
                </button>
              </div>
            </div>

            <div class="sw-row">
              <span class="sw-label">{{ t("settings.appearance") }}</span>
              <div class="segmented" role="group" :aria-label="t('settings.appearance')">
                <button
                  v-for="option in APPEARANCE_OPTIONS"
                  :key="option"
                  type="button"
                  :class="{ on: current.appearance === option }"
                  :aria-pressed="current.appearance === option"
                  @click="commitSelect('appearance', option)"
                >
                  {{ t(APPEARANCE_LABEL[option]) }}
                </button>
              </div>
            </div>
          </div>
        </section>

        <section class="sw-group">
          <h2>{{ t("settings.usageAndPricing") }}</h2>
          <div class="card sw-card">
            <div class="sw-row sw-row--action">
              <span class="sw-label">
                {{ t("settings.pricingCatalog") }}
                <span class="sw-desc">{{ t("settings.pricingCatalogDescription") }}</span>
              </span>
              <button
                type="button"
                class="flat-btn"
                :disabled="pricingRefreshPending"
                @click="updatePricingCatalog"
              >
                {{
                  pricingRefreshPending
                    ? t("settings.pricingCatalogUpdating")
                    : t("settings.pricingCatalogUpdate")
                }}
              </button>
            </div>
            <p
              v-if="pricingRefreshState !== 'idle'"
              class="settings__action-status supporting"
              :class="{ 'settings__action-status--error': pricingRefreshState === 'failure' }"
              aria-live="polite"
            >
              {{
                pricingRefreshState === "success"
                  ? t("settings.pricingCatalogUpdated")
                  : pricingRefreshState === "partial"
                    ? t("settings.pricingCatalogPartiallyUpdated")
                    : t("settings.pricingCatalogUpdateFailed")
              }}
            </p>
            <div class="sw-row sw-row--action">
              <span class="sw-label">
                {{ t("settings.rebuildUsage") }}
                <span class="sw-desc">{{ t("settings.rebuildUsageDescription") }}</span>
              </span>
              <button
                type="button"
                class="flat-btn"
                data-rebuild-btn
                :class="{ 'flat-btn--danger': rebuildState === 'confirm' }"
                :disabled="rebuildPending"
                @click="requestRebuild"
              >
                {{ rebuildLabel }}
              </button>
            </div>
            <p
              v-if="
                rebuildState === 'running' ||
                rebuildState === 'success' ||
                rebuildState === 'failure'
              "
              class="settings__action-status supporting"
              :class="{ 'settings__action-status--error': rebuildState === 'failure' }"
              aria-live="polite"
            >
              {{ rebuildStatusText }}
            </p>
          </div>
        </section>

        <section class="sw-group">
          <h2>{{ t("settings.statsServices") }}</h2>
          <p class="supporting settings__group-description">
            {{ t("settings.statsServicesDescription") }}
          </p>
          <div class="card sw-card">
            <div
              v-for="source in STATS_SERVICE_SOURCES"
              :key="source"
              class="sw-row sw-row--toggle"
            >
              <span class="sw-label">{{ t(`provider.${source}`) }}</span>
              <button
                type="button"
                class="toggle"
                :class="{ off: !current.usageServiceVisibility[source] }"
                role="switch"
                :aria-checked="current.usageServiceVisibility[source]"
                @click="commitStatsService(source, !current.usageServiceVisibility[source])"
              >
                <span class="visually-hidden">{{ t(`provider.${source}`) }}</span>
              </button>
            </div>
          </div>
        </section>

        <section class="sw-group">
          <h2>{{ t("settings.about") }}</h2>
          <div class="card sw-card sw-about">
            <p class="settings__version numeric">
              {{ t("settings.version", { version: settings.version }) }}
            </p>
            <p class="settings__privacy supporting">{{ t("settings.privacy") }}</p>
          </div>
        </section>
      </template>
    </div>
  </main>
</template>

<style scoped>
.settings {
  min-block-size: 100vh;
  padding: clamp(1.5rem, 4vw, 2.5rem);
  background: var(--surface-primary);
}

.settings__inner {
  display: grid;
  align-content: start;
  gap: var(--space-6);
  inline-size: min(100%, 40rem);
  margin-inline: auto;
}

.settings__header {
  display: grid;
  justify-items: start;
  gap: var(--space-4);
  padding-block-end: 0.75rem;
  margin-block-end: 0.5rem;
  border-block-end: 1px solid var(--border-subtle);
}

h1 {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 680;
  letter-spacing: -0.025em;
  line-height: 1.15;
}

h1[tabindex="-1"]:focus {
  outline: none;
}

.sw-group {
  display: grid;
  gap: var(--space-3);
}

.sw-group > h2 {
  margin: 0 0 0 0.125rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 680;
  letter-spacing: 0.04em;
}

.sw-card {
  padding: 0.25rem 0;
  background: var(--surface-raised);
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
}

.sw-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.75rem 1rem;
  min-block-size: 3rem;
}

.sw-row + .sw-row {
  border-block-start: 1px solid color-mix(in srgb, var(--border-subtle) 65%, transparent);
}

.sw-row--action {
  align-items: start;
}

.sw-label {
  display: grid;
  gap: 0.25rem;
  font-size: 0.8125rem;
  font-weight: 550;
  line-height: 1.35;
}

.sw-desc {
  color: var(--text-secondary);
  font-size: 0.71875rem;
  font-weight: 400;
  line-height: 1.5;
  max-inline-size: 22.5rem;
}

select {
  min-block-size: 2rem;
  padding: 0 var(--space-2);
  color: var(--text-primary);
  background-color: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
}

.segmented {
  display: inline-flex;
  flex: none;
  padding: 0.1875rem;
  border-radius: 0.5625rem;
  background: var(--track-background);
}

.segmented button {
  min-block-size: 1.625rem;
  padding: 0 0.6875rem;
  border: 0;
  border-radius: 0.375rem;
  color: var(--text-secondary);
  background: transparent;
  font-size: 0.6875rem;
  white-space: nowrap;
}

.segmented button:hover {
  color: var(--text-primary);
}

.segmented button.on {
  color: var(--text-primary);
  background: var(--surface-raised);
  box-shadow: 0 1px 3px rgb(16 16 20 / 12%);
  font-weight: 570;
}

/* 40 × 24 开关（产物 toggle），off 态用轨道色 */
.toggle {
  position: relative;
  flex: none;
  inline-size: 2.5rem;
  block-size: 1.5rem;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: var(--status-success);
  cursor: pointer;
}

.toggle::after {
  content: "";
  position: absolute;
  inset-block-start: 0.125rem;
  inset-inline-end: 0.125rem;
  inline-size: 1.25rem;
  block-size: 1.25rem;
  border-radius: 50%;
  background: #ffffff;
  box-shadow: 0 1px 3px rgb(0 0 0 / 25%);
  transition: inset-inline-start var(--motion-fast) var(--ease-out);
}

.toggle.off {
  background: var(--track-background);
}

.toggle.off::after {
  inset-inline-end: auto;
  inset-inline-start: 0.125rem;
}

.toggle:hover {
  filter: brightness(1.05);
}

.flat-btn {
  flex: none;
  min-block-size: 2.25rem;
  padding: 0 0.875rem;
  border: 0;
  border-radius: 0.5625rem;
  color: var(--text-primary);
  background: var(--track-background);
  font-size: 0.75rem;
  font-weight: 570;
  white-space: nowrap;
}

.flat-btn:hover {
  background: color-mix(in srgb, var(--text-primary) 12%, var(--track-background));
}

.flat-btn--danger,
.flat-btn--danger:hover {
  color: var(--status-error);
  background: color-mix(in srgb, var(--status-error) 12%, var(--track-background));
}

.flat-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.settings__group-description {
  margin: 0;
  font-size: 0.71875rem;
  line-height: 1.5;
}

.settings__action-status {
  margin: 0;
  padding: 0 1rem 0.75rem;
  font-size: 0.71875rem;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.settings__action-status--error {
  color: var(--status-error);
}

.settings__error {
  display: grid;
  gap: var(--space-1);
  margin: 0;
  padding: var(--space-3) var(--space-4);
  color: var(--status-error);
  background: var(--surface-raised);
  border: 1px solid var(--status-error);
  border-radius: var(--radius-small);
  font-size: 0.8125rem;
}

.settings__error span {
  color: var(--text-secondary);
}

.sw-about {
  display: grid;
  gap: var(--space-2);
  padding: 0.75rem 1rem;
}

.settings__version {
  margin: 0;
  font-size: 0.8125rem;
}

.settings__privacy {
  margin: 0;
  font-size: 0.71875rem;
  line-height: 1.6;
}

@media (prefers-reduced-motion: no-preference) {
  .segmented button,
  .toggle,
  .flat-btn {
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      filter var(--motion-fast) var(--ease-out),
      scale var(--motion-fast) var(--ease-out);
  }

  .segmented button:active,
  .toggle:active,
  .flat-btn:active {
    scale: 0.96;
  }
}
</style>
