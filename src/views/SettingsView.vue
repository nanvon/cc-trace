<script setup lang="ts">
/**
 * 主窗口设置视图：只负责用户可以安全改变的应用偏好。
 *
 * 不承载额度详情、账号操作或 Provider 登录，见 `docs/信息架构与核心流程.md` 第 7 节。
 * 保存成功立即生效；写入失败时**保留原值**并明确提示。
 */
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";

import { navigateMain } from "../features/app/navigation";
import { refreshPricingCatalog } from "../features/usage/api";
import {
  APPEARANCE_OPTIONS,
  LANGUAGE_OPTIONS,
  REFRESH_INTERVAL_OPTIONS,
  type AppearancePreference,
  type LanguagePreference,
  type RefreshIntervalOption,
  type SettingsUpdate,
} from "../features/settings/contracts";
import { useSettingsStore } from "../features/settings/store";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const settings = useSettingsStore();
const pricingRefreshState = ref<"idle" | "success" | "partial" | "failure">("idle");
const pricingRefreshPending = ref(false);
const PRICING_REFRESH_STATE = {
  complete: "success",
  partial: "partial",
  failed: "failure",
} as const;

const current = computed(() => settings.settings);

const INTERVAL_LABEL: Record<RefreshIntervalOption, string> = {
  "15m": "settings.intervalOption.m15",
  "30m": "settings.intervalOption.m30",
  "60m": "settings.intervalOption.m60",
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
async function commit(event: Event, key: "refreshInterval" | "language" | "appearance") {
  const select = event.target as HTMLSelectElement;
  await settings.update({ [key]: select.value } as SettingsUpdate);
  if (current.value) {
    select.value = current.value[key];
  }
}

async function commitLaunchAtLogin(event: Event) {
  const checkbox = event.target as HTMLInputElement;
  await settings.update({ launchAtLogin: checkbox.checked });
  if (current.value) {
    checkbox.checked = current.value.launchAtLogin;
  }
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

function returnToUsage(): void {
  const focusTarget = route.query.origin === "quota" ? "settings-trigger" : "usage-title";
  void navigateMain(router, "quota", focusTarget);
}
</script>

<template>
  <main class="settings">
    <div class="settings__inner">
      <header class="settings__header">
        <button type="button" class="button button--quiet settings__back" @click="returnToUsage">
          <span aria-hidden="true">←</span>
          {{ t("settings.backToUsage") }}
        </button>
        <h1 id="main-settings-title" tabindex="-1">{{ t("settings.title") }}</h1>
      </header>

      <template v-if="current">
        <p v-if="settings.writeFailed" class="settings__error" role="alert">
          <strong>{{ t("error.settingsWriteFailed.title") }}</strong>
          <span>{{ t("error.settingsWriteFailed.nextStep") }}</span>
        </p>

        <section class="settings__group">
          <h2>{{ t("settings.general") }}</h2>

          <label class="settings__row">
            <span>{{ t("settings.refreshInterval") }}</span>
            <select
              name="refresh-interval"
              :value="current.refreshInterval"
              autocomplete="off"
              @change="commit($event, 'refreshInterval')"
            >
              <option v-for="option in REFRESH_INTERVAL_OPTIONS" :key="option" :value="option">
                {{ t(INTERVAL_LABEL[option]) }}
              </option>
            </select>
          </label>

          <label class="settings__row settings__row--check">
            <input
              type="checkbox"
              name="launch-at-login"
              :checked="current.launchAtLogin"
              @change="commitLaunchAtLogin($event)"
            />
            <span>{{ t("settings.launchAtLogin") }}</span>
          </label>
        </section>

        <section class="settings__group">
          <h2>{{ t("settings.usageAndPricing") }}</h2>

          <div class="settings__row settings__row--action">
            <span class="settings__action-copy">
              <span>{{ t("settings.pricingCatalog") }}</span>
              <span class="supporting">{{ t("settings.pricingCatalogDescription") }}</span>
            </span>
            <button
              type="button"
              class="button button--flat"
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
        </section>

        <section class="settings__group">
          <h2>{{ t("settings.appearanceAndLanguage") }}</h2>

          <label class="settings__row">
            <span>{{ t("settings.language") }}</span>
            <select
              name="language"
              :value="current.language"
              autocomplete="off"
              @change="commit($event, 'language')"
            >
              <option v-for="option in LANGUAGE_OPTIONS" :key="option" :value="option">
                {{ t(LANGUAGE_LABEL[option]) }}
              </option>
            </select>
          </label>

          <label class="settings__row">
            <span>{{ t("settings.appearance") }}</span>
            <select
              name="appearance"
              :value="current.appearance"
              autocomplete="off"
              @change="commit($event, 'appearance')"
            >
              <option v-for="option in APPEARANCE_OPTIONS" :key="option" :value="option">
                {{ t(APPEARANCE_LABEL[option]) }}
              </option>
            </select>
          </label>
        </section>

        <section class="settings__group">
          <h2>{{ t("settings.about") }}</h2>
          <p class="settings__version numeric">
            {{ t("settings.version", { version: settings.version }) }}
          </p>
          <p class="settings__privacy supporting">{{ t("settings.privacy") }}</p>
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
  inline-size: min(100%, 32.5rem);
  margin-inline: auto;
}

.settings__header {
  display: grid;
  justify-items: start;
  gap: var(--space-4);
}

.settings__back {
  margin-inline-start: calc(var(--space-4) * -1);
}

h1 {
  margin: 0;
  font-size: 1.5rem;
  font-weight: 620;
  letter-spacing: -0.02em;
}

h2 {
  margin: 0;
  font-size: 0.875rem;
  font-weight: 620;
}

.settings__group {
  display: grid;
  gap: var(--space-4);
  padding-block-start: var(--space-5);
  border-block-start: 1px solid var(--border-subtle);
}

.settings__row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(9rem, auto);
  align-items: center;
  gap: var(--space-4);
  min-block-size: 2.5rem;
}

.settings__row--check {
  display: flex;
  gap: var(--space-3);
}

.settings__row--action {
  align-items: start;
}

.settings__action-copy {
  display: grid;
  gap: var(--space-1);
  min-inline-size: 0;
}

.settings__action-copy .supporting,
.settings__action-status {
  font-size: 0.8125rem;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.settings__action-status {
  margin: calc(var(--space-2) * -1) 0 0;
}

.settings__action-status--error {
  color: var(--status-error);
}

select {
  min-block-size: 2.5rem;
  padding: 0 var(--space-2);
  color: var(--text-primary);
  background-color: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
}

input[type="checkbox"] {
  inline-size: 1rem;
  block-size: 1rem;
  accent-color: var(--action-primary);
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

.settings__version {
  margin: 0;
  font-size: 0.8125rem;
}

.settings__privacy {
  margin: 0;
  font-size: 0.8125rem;
  line-height: 1.6;
}

@media (max-width: 520px) {
  .settings__row:not(.settings__row--check) {
    grid-template-columns: 1fr;
    gap: var(--space-2);
  }

  select {
    inline-size: 100%;
  }

  .settings__row--action {
    grid-template-columns: 1fr;
    gap: var(--space-3);
  }

  .settings__row--action .button {
    inline-size: 100%;
  }
}
</style>
