<script setup lang="ts">
/**
 * 首次启动：四步，对齐 cc-bar Onboarding（欢迎 → 检测账号 → 选择显示方式 → 就绪）。
 *
 * 只做首次使用必要事项——说明用途与凭据边界、预告系统授权、展示 Provider 发现结果、
 * 让用户选系统区域与悬浮窗。不在应用内登录，不要求用户修复无凭据状态，也不读取或迁移
 * cc-bar 的任何数据。账号类展示经 `usePrivacy()` 遮挡。
 */
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";

import symbolUrl from "../assets/brand/cc-trace-symbol.svg";
import { useAppShell } from "../app/shell";
import { openCompactPanel, openMainWindow } from "../features/app/windows";
import { getCredentialSources, type CredentialSources } from "../features/settings/maintenance";
import type { ServiceSettings } from "../features/settings/contracts";
import { providerLabel } from "../lib/labels";
import { usePrivacy } from "../lib/privacy";
import { presentProvider } from "../lib/status";

const { t } = useI18n();
const { quota, settings, closeSurface } = useAppShell("onboarding");
const privacy = usePrivacy();
const initialCheckStarted = ref(false);
const step = ref(0);
const STEP_COUNT = 4;
const sources = ref<CredentialSources | null>(null);

type QuotaService = "codex" | "claude" | "antigravity" | "cursor" | "commandCode";
const MENU_BAR_SERVICES: readonly QuotaService[] = [
  "codex",
  "claude",
  "antigravity",
  "cursor",
  "commandCode",
] as const;

/** 只列主账号：导入账号是设置里的进阶操作，引导不展示。 */
const checks = computed(() =>
  quota.visible
    .filter((provider) => provider.kind === "primary")
    .map((provider) => {
      const presentation = presentProvider(provider);
      const waitingForCheck =
        !initialCheckStarted.value &&
        provider.refresh === "idle" &&
        provider.freshness === "empty" &&
        provider.availability === "ready";
      const details: string[] = [];
      if (provider.identity?.account) details.push(privacy.account(provider.identity.account));
      if (provider.identity?.plan) details.push(provider.identity.plan);
      if (sources.value) {
        if (provider.provider === "claude") {
          details.push(t(`settings.credentialSource.claudeCode.${sources.value.claudeCode}`));
          if (sources.value.claudeDesktop) {
            details.push(t("settings.credentialSource.desktopAvailable"));
          }
        } else if (provider.provider === "commandCode") {
          details.push(t(`settings.credentialSource.commandCode.${sources.value.commandCode}`));
        }
      }

      return {
        id: provider.provider,
        name: providerLabel(t, provider.provider),
        details: details.join(" · "),
        presentation: waitingForCheck
          ? { ...presentation, titleKey: "onboarding.notChecked", tone: "neutral" as const }
          : presentation,
      };
    }),
);

/** 任一 Provider 没有凭据时，明确告诉用户仍然可以继续。 */
const showsNoCredentialsHint = computed(() =>
  quota.visible.some((provider) => provider.availability === "no_credentials"),
);

const showsKeychainNotice = computed(() => settings.status?.platform === "macos");

async function checkProviders(): Promise<void> {
  initialCheckStarted.value = true;
  await quota.refresh();
  try {
    sources.value = await getCredentialSources();
  } catch {
    sources.value = null;
  }
}

function menuBarOn(service: QuotaService): boolean {
  return settings.settings?.services[service].menuBar ?? false;
}

/** 打开某个服务的菜单栏显示时同时打开额度，否则它没有数据可显示（与 cc-bar 一致）。 */
async function toggleMenuBar(service: QuotaService): Promise<void> {
  const services = settings.settings?.services;
  if (!services) return;
  const next = structuredClone(services);
  const entry: ServiceSettings = next[service];
  entry.menuBar = !entry.menuBar;
  if (entry.menuBar) entry.quota = true;
  await settings.update({ services: next });
  if (entry.menuBar) await quota.refresh();
}

async function toggleHud(): Promise<void> {
  const current = settings.settings;
  if (!current) return;
  await settings.update({ hud: { ...current.hud, enabled: !current.hud.enabled } });
}

async function finish(openMain: boolean): Promise<void> {
  // 收尾本身也是明确的用户动作；若跳过了单独检查，先启动首次刷新，
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
  if (openMain) {
    await openMainWindow();
  } else {
    await openCompactPanel();
  }
}
</script>

<template>
  <main class="onboarding">
    <p class="onboarding__step">
      {{ t("onboarding.stepOf", { current: step + 1, total: STEP_COUNT }) }}
    </p>

    <!-- 1 欢迎 -->
    <template v-if="step === 0">
      <header class="onboarding__intro">
        <img :src="symbolUrl" width="40" height="40" alt="" />
        <h1>{{ t("onboarding.title") }}</h1>
        <p>{{ t("onboarding.intro") }}</p>
        <p class="supporting">{{ t("onboarding.residency") }}</p>
      </header>
      <section class="onboarding__section">
        <h2 class="utility-label">{{ t("onboarding.boundaryHeading") }}</h2>
        <p class="supporting">{{ t("onboarding.boundary") }}</p>
        <p v-if="showsKeychainNotice" class="supporting onboarding__hint">
          {{ t("onboarding.keychainNotice") }}
        </p>
      </section>
    </template>

    <!-- 2 检测账号 -->
    <section v-else-if="step === 1" class="onboarding__section">
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
          <span class="onboarding__detail">
            <span class="onboarding__provider" translate="no">{{ check.name }}</span>
            <small v-if="check.details">{{ check.details }}</small>
          </span>
          <span class="onboarding__state">{{ t(check.presentation.titleKey) }}</span>
        </li>
      </ul>
      <p v-if="showsNoCredentialsHint" class="supporting onboarding__hint">
        {{ t("onboarding.noCredentialsHint") }}
      </p>
    </section>

    <!-- 3 选择显示方式 -->
    <template v-else-if="step === 2">
      <section class="onboarding__section">
        <h2 class="utility-label">{{ t("onboarding.menuBarHeading") }}</h2>
        <p class="supporting">{{ t("onboarding.menuBarDescription") }}</p>
        <div class="onboarding__toggles">
          <button
            v-for="service in MENU_BAR_SERVICES"
            :key="service"
            type="button"
            class="onboarding__toggle"
            role="switch"
            :aria-checked="menuBarOn(service)"
            @click="toggleMenuBar(service)"
          >
            <span translate="no">{{ providerLabel(t, service) }}</span>
          </button>
        </div>
      </section>
      <section class="onboarding__section">
        <h2 class="utility-label">{{ t("onboarding.hudHeading") }}</h2>
        <p class="supporting">{{ t("onboarding.hudDescription") }}</p>
        <div class="onboarding__toggles">
          <button
            type="button"
            class="onboarding__toggle"
            role="switch"
            :aria-checked="settings.settings?.hud.enabled ?? false"
            @click="toggleHud"
          >
            {{ t("onboarding.hudEnabled") }}
          </button>
        </div>
      </section>
    </template>

    <!-- 4 就绪 -->
    <header v-else class="onboarding__intro">
      <img :src="symbolUrl" width="40" height="40" alt="" />
      <h1>{{ t("onboarding.readyTitle") }}</h1>
      <p>{{ t("onboarding.readyBody") }}</p>
    </header>

    <p v-if="settings.writeFailed" class="onboarding__error" role="alert">
      {{ t("error.settingsWriteFailed.title") }} · {{ t("error.settingsWriteFailed.nextStep") }}
    </p>

    <footer class="onboarding__actions">
      <button v-if="step > 0" type="button" class="button button--quiet" @click="step -= 1">
        {{ t("onboarding.back") }}
      </button>
      <button v-else type="button" class="button button--quiet" @click="closeSurface">
        {{ t("onboarding.later") }}
      </button>
      <button
        v-if="step < STEP_COUNT - 1"
        type="button"
        class="button button--primary"
        :disabled="!settings.status"
        @click="step += 1"
      >
        {{ step === 0 ? t("onboarding.getStarted") : t("onboarding.next") }}
      </button>
      <span v-else class="onboarding__toggles">
        <button
          type="button"
          class="button button--quiet"
          :disabled="!settings.status"
          @click="finish(false)"
        >
          {{ t("onboarding.done") }}
        </button>
        <button
          type="button"
          class="button button--primary"
          :disabled="!settings.status"
          @click="finish(true)"
        >
          {{ t("onboarding.openMain") }}
        </button>
      </span>
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

.onboarding__toggles {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.onboarding__toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
  font-size: 0.8125rem;
}

.onboarding__toggle[aria-checked="false"] {
  color: var(--text-secondary);
}

.onboarding__detail {
  display: grid;
  gap: 0.125rem;
  min-inline-size: 0;
}

.onboarding__detail small {
  color: var(--text-secondary);
  font-size: 0.75rem;
  overflow-wrap: anywhere;
}

.onboarding__step {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.75rem;
}

.onboarding__actions {
  justify-content: space-between;
}
</style>
