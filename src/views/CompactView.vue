<script setup lang="ts">
/**
 * 紧凑额度面板：回答「现在额度紧不紧」。
 *
 * 只承载总体状态、两个 Provider 概览、刷新、主窗口入口、设置与退出。
 * 完整错误诊断、历史与设置表单都不在这里，见 `docs/信息架构与核心流程.md` 第 5 节。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import { useAppShell } from "../app/shell";
import OverallSignal from "../components/OverallSignal.vue";
import ProviderLane from "../components/ProviderLane.vue";
import RefreshIcon from "../components/RefreshIcon.vue";
import { openMainWindow, openSettingsWindow, quitApp } from "../features/app/windows";
import { presentOverall } from "../lib/status";

const { t } = useI18n();
const { quota, settings } = useAppShell("compact");

const refreshButton = ref<HTMLButtonElement | null>(null);
const entered = ref(false);

/** 面板从系统区域图标的方向产生：macOS 在图标下方，Windows 在托盘上方。 */
const origin = computed(() => (settings.status?.platform === "macos" ? "top" : "bottom"));

/** 播报总体状态，不逐个 Provider 重复播报无变化的刷新结果。 */
const liveMessage = computed(() => {
  const leader = presentOverall(quota.ordered);
  return leader ? t(leader.presentation.titleKey) : "";
});

/**
 * 窗口是复用的：隐藏后再次显示不会重新挂载组件，因此入场动效与初始焦点
 * 都绑定在窗口的 focus 上。
 */
function handleWindowFocus(): void {
  entered.value = false;
  requestAnimationFrame(() => {
    entered.value = true;
  });
  refreshButton.value?.focus();
}

onMounted(() => {
  document.body.dataset.surface = "compact";
  window.addEventListener("focus", handleWindowFocus);
  handleWindowFocus();
});

onBeforeUnmount(() => {
  window.removeEventListener("focus", handleWindowFocus);
});
</script>

<template>
  <main class="panel" :class="{ 'panel--entered': entered }" :data-origin="origin">
    <p class="visually-hidden" aria-live="polite">{{ liveMessage }}</p>

    <OverallSignal :providers="quota.ordered" variant="compact">
      <template #actions>
        <button
          ref="refreshButton"
          type="button"
          class="button button--quiet button--icon"
          :aria-label="t('a11y.refreshAll')"
          :title="t('common.refresh')"
          @click="quota.refresh()"
        >
          <RefreshIcon :spinning="quota.busy" />
        </button>
      </template>
    </OverallSignal>

    <section class="panel__lanes" :aria-label="t('a11y.statusRegion')">
      <ProviderLane
        v-for="provider in quota.ordered"
        :key="provider.provider"
        :provider="provider"
        variant="compact"
      />
    </section>

    <footer class="panel__actions">
      <button type="button" class="button button--primary" @click="openMainWindow">
        {{ t("common.details") }}
      </button>
      <button type="button" class="button button--quiet" @click="openSettingsWindow">
        {{ t("common.settings") }}
      </button>
      <button type="button" class="button button--quiet panel__quit" @click="quitApp">
        {{ t("common.quit") }}
      </button>
    </footer>
  </main>
</template>

<style scoped>
.panel {
  display: grid;
  grid-template-rows: auto 1fr auto auto;
  gap: var(--space-4);
  block-size: 100vh;
  padding: var(--space-4);
  /* 平台材质属于窗口外壳；内容层保持实色以保证对比度可靠 */
  background: var(--surface-primary);
  border: 1px solid var(--ring-panel);
  border-radius: var(--radius-shell);
  overflow: hidden;
}

.panel__lanes {
  display: grid;
  align-content: start;
  gap: var(--space-3);
}

.panel__actions {
  display: flex;
  gap: var(--space-2);
}

.panel__quit {
  margin-inline-start: auto;
  color: var(--text-secondary);
}

@media (prefers-reduced-motion: no-preference) {
  .panel {
    opacity: 0;
    scale: 0.985;
    transition:
      opacity var(--motion-panel) var(--ease-out),
      scale var(--motion-panel) var(--ease-out);
  }

  .panel[data-origin="top"] {
    transform-origin: top center;
  }

  .panel[data-origin="bottom"] {
    transform-origin: bottom center;
  }

  .panel--entered {
    opacity: 1;
    scale: 1;
  }
}
</style>
