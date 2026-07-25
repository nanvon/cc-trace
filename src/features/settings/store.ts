import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";

import { applyAppearance, applyLocale, resolveLocale, type AppLocale } from "../../i18n";
import {
  completeOnboarding as completeOnboardingCommand,
  getAppStatus,
  readSettings,
  updateSettings,
} from "./api";
import type { AppStatus, Settings, SettingsUpdate } from "./contracts";

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<Settings | null>(null);
  const status = ref<AppStatus | null>(null);
  /** 上一次写入失败。界面据此提示，并保持显示原值。 */
  const writeFailed = ref(false);

  const locale = computed<AppLocale>(() =>
    settings.value && status.value
      ? resolveLocale(settings.value.language, status.value.systemLocale)
      : "en",
  );

  const version = computed(() => status.value?.version ?? "");
  const onboardingCompleted = computed(() => settings.value?.onboarding.completed ?? true);

  // 语言与外观是纯展示偏好，设置一到达就立即生效，不需要重启窗口。
  watch(locale, applyLocale, { immediate: true });
  watch(
    () => settings.value?.appearance,
    (appearance) => {
      if (appearance) {
        applyAppearance(appearance);
      }
    },
    { immediate: true },
  );

  async function load(): Promise<void> {
    const [nextSettings, nextStatus] = await Promise.all([readSettings(), getAppStatus()]);
    settings.value = nextSettings;
    status.value = nextStatus;
  }

  /** 采纳来自 `settings://updated` 的推送，让多个窗口保持一致。 */
  function adopt(next: Settings): void {
    settings.value = next;
  }

  async function update(patch: SettingsUpdate): Promise<void> {
    try {
      settings.value = await updateSettings(patch);
      writeFailed.value = false;
    } catch {
      // 写入失败：Rust 已保留原值，这里不做乐观更新，界面继续显示旧选项。
      writeFailed.value = true;
    }
  }

  async function completeOnboarding(): Promise<boolean> {
    try {
      settings.value = await completeOnboardingCommand();
      writeFailed.value = false;
      return true;
    } catch {
      writeFailed.value = true;
      return false;
    }
  }

  return {
    settings,
    status,
    writeFailed,
    locale,
    version,
    onboardingCompleted,
    load,
    adopt,
    update,
    completeOnboarding,
  };
});
