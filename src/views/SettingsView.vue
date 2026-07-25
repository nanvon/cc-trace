<script setup lang="ts">
/**
 * 设置窗口：只负责用户可以安全改变的应用偏好。
 *
 * 不承载额度详情、账号操作或 Provider 登录，见 `docs/信息架构与核心流程.md` 第 7 节。
 * 保存成功立即生效；写入失败时**保留原值**并明确提示。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import { useAppShell } from "../app/shell";
import {
  APPEARANCE_OPTIONS,
  LANGUAGE_OPTIONS,
  REFRESH_INTERVAL_OPTIONS,
  type AppearancePreference,
  type LanguagePreference,
  type RefreshIntervalOption,
  type SettingsUpdate,
} from "../features/settings/contracts";

const { t } = useI18n();
const { settings } = useAppShell("settings");

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
</script>

<template>
  <main v-if="current" class="settings">
    <header>
      <h1>{{ t("settings.title") }}</h1>
    </header>

    <p v-if="settings.writeFailed" class="settings__error" role="alert">
      <strong>{{ t("error.settingsWriteFailed.title") }}</strong>
      <span>{{ t("error.settingsWriteFailed.nextStep") }}</span>
    </p>

    <section class="settings__group">
      <h2>{{ t("settings.general") }}</h2>

      <label class="settings__row">
        <span>{{ t("settings.refreshInterval") }}</span>
        <select
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
          :checked="current.launchAtLogin"
          @change="commitLaunchAtLogin($event)"
        />
        <span>{{ t("settings.launchAtLogin") }}</span>
      </label>
    </section>

    <section class="settings__group">
      <h2>{{ t("settings.appearanceAndLanguage") }}</h2>

      <label class="settings__row">
        <span>{{ t("settings.language") }}</span>
        <select :value="current.language" autocomplete="off" @change="commit($event, 'language')">
          <option v-for="option in LANGUAGE_OPTIONS" :key="option" :value="option">
            {{ t(LANGUAGE_LABEL[option]) }}
          </option>
        </select>
      </label>

      <label class="settings__row">
        <span>{{ t("settings.appearance") }}</span>
        <select
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
  </main>
</template>

<style scoped>
.settings {
  display: grid;
  align-content: start;
  gap: var(--space-6);
  min-block-size: 100vh;
  padding: var(--space-6);
  background: var(--surface-primary);
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

select {
  min-block-size: 2.25rem;
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
</style>
