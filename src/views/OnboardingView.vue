<script setup lang="ts">
/**
 * 首次启动：建立信任并检查环境。
 *
 * 只做首次使用必要事项——说明用途与凭据边界、预告系统授权、展示 Provider 发现结果。
 * 不在应用内登录，不要求用户修复无凭据状态，也不读取或迁移 cc-bar 的任何数据。
 */
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";

import symbolUrl from "../assets/brand/cc-trace-symbol.svg";
import { useAppShell } from "../app/shell";
import { openCompactPanel } from "../features/app/windows";
import { providerLabel } from "../lib/labels";
import { presentProvider } from "../lib/status";

const { t } = useI18n();
const { quota, settings, closeSurface } = useAppShell("onboarding");
const initialCheckStarted = ref(false);

const checks = computed(() =>
  quota.ordered.map((provider) => {
    const presentation = presentProvider(provider);
    const waitingForCheck =
      !initialCheckStarted.value &&
      provider.refresh === "idle" &&
      provider.freshness === "empty" &&
      provider.availability === "ready";

    return {
      id: provider.provider,
      name: providerLabel(t, provider.provider),
      presentation: waitingForCheck
        ? { ...presentation, titleKey: "onboarding.notChecked", tone: "neutral" as const }
        : presentation,
    };
  }),
);

/** 任一 Provider 没有凭据时，明确告诉用户仍然可以继续。 */
const showsNoCredentialsHint = computed(() =>
  quota.ordered.some((provider) => provider.availability === "no_credentials"),
);

const showsKeychainNotice = computed(() => settings.status?.platform === "macos");

async function checkProviders(): Promise<void> {
  initialCheckStarted.value = true;
  await quota.refresh();
}

async function finish(): Promise<void> {
  // “开始使用”本身也是明确的用户动作；若跳过了单独检查，先启动首次刷新，
  // 避免进入紧凑面板后长时间停在尚未检查的空态。
  if (!initialCheckStarted.value) {
    await checkProviders();
  }

  const saved = await settings.completeOnboarding();
  if (!saved) {
    // 写入失败：保持未完成状态，下次启动继续引导，不假装已完成。
    return;
  }
  await closeSurface();
  await openCompactPanel();
}
</script>

<template>
  <main class="onboarding">
    <header class="onboarding__intro">
      <img :src="symbolUrl" width="40" height="40" alt="" />
      <h1>{{ t("onboarding.title") }}</h1>
      <p>{{ t("onboarding.intro") }}</p>
      <p class="supporting">{{ t("onboarding.residency") }}</p>
    </header>

    <section class="onboarding__section">
      <div class="onboarding__section-heading">
        <h2 class="utility-label">{{ t("onboarding.checkHeading") }}</h2>
        <button
          type="button"
          class="button button--quiet"
          :disabled="!settings.status || quota.busy"
          @click="checkProviders"
        >
          {{
            t(
              quota.busy
                ? "onboarding.checking"
                : initialCheckStarted
                  ? "onboarding.checkAgain"
                  : "onboarding.checkNow",
            )
          }}
        </button>
      </div>
      <ul class="onboarding__checks" aria-live="polite" :aria-busy="quota.busy">
        <li v-for="check in checks" :key="check.id" :class="`tone-${check.presentation.tone}`">
          <span class="onboarding__provider" translate="no">{{ check.name }}</span>
          <span class="onboarding__state">{{ t(check.presentation.titleKey) }}</span>
        </li>
      </ul>
      <p v-if="showsNoCredentialsHint" class="supporting onboarding__hint">
        {{ t("onboarding.noCredentialsHint") }}
      </p>
    </section>

    <section class="onboarding__section">
      <h2 class="utility-label">{{ t("onboarding.boundaryHeading") }}</h2>
      <p class="supporting">{{ t("onboarding.boundary") }}</p>
      <p v-if="showsKeychainNotice" class="supporting onboarding__hint">
        {{ t("onboarding.keychainNotice") }}
      </p>
    </section>

    <p v-if="settings.writeFailed" class="onboarding__error" role="alert">
      {{ t("error.settingsWriteFailed.title") }} · {{ t("error.settingsWriteFailed.nextStep") }}
    </p>

    <footer class="onboarding__actions">
      <button
        type="button"
        class="button button--primary"
        :disabled="!settings.status"
        @click="finish"
      >
        {{ t("onboarding.done") }}
      </button>
      <button type="button" class="button button--quiet" @click="closeSurface">
        {{ t("onboarding.later") }}
      </button>
    </footer>
  </main>
</template>

<style scoped>
.onboarding {
  display: grid;
  align-content: start;
  gap: var(--space-6);
  min-block-size: 100vh;
  padding: var(--space-8) var(--space-6) var(--space-6);
  background: var(--surface-primary);
}

.onboarding__intro {
  display: grid;
  gap: var(--space-3);
  justify-items: start;
}

.onboarding__intro img {
  margin-block-end: var(--space-1);
}

h1 {
  margin: 0;
  font-size: 1.625rem;
  font-weight: 620;
  letter-spacing: -0.025em;
}

.onboarding__intro p {
  margin: 0;
  max-inline-size: 34rem;
  line-height: 1.6;
}

.onboarding__section {
  display: grid;
  gap: var(--space-3);
  padding-block-start: var(--space-5);
  border-block-start: 1px solid var(--border-subtle);
}

.onboarding__section p {
  margin: 0;
  max-inline-size: 34rem;
  font-size: 0.875rem;
  line-height: 1.6;
}

.onboarding__section-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
}

.onboarding__checks {
  display: grid;
  gap: var(--space-2);
  margin: 0;
  padding: 0;
  list-style: none;
}

.onboarding__checks li {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-3) var(--space-4);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
}

.onboarding__provider {
  font-weight: 560;
}

.onboarding__state {
  color: var(--text-secondary);
  font-size: 0.8125rem;
}

.tone-warning .onboarding__state {
  color: var(--status-warning);
}

.tone-critical .onboarding__state {
  color: var(--status-error);
}

.onboarding__hint {
  font-size: 0.8125rem;
}

.onboarding__error {
  margin: 0;
  color: var(--status-error);
  font-size: 0.8125rem;
}

.onboarding__actions {
  display: flex;
  gap: var(--space-2);
  padding-block-start: var(--space-2);
}
</style>
