<script setup lang="ts">
/**
 * 主窗口：无侧边栏、无 Tab 的单页额度总览。
 *
 * 它只解释当前额度、数据可信度和恢复操作，不承担「更多功能的容器」，
 * 见 `docs/信息架构与核心流程.md` 第 6 节。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";

import DevScenarioBar from "../components/DevScenarioBar.vue";
import OverallSignal from "../components/OverallSignal.vue";
import ProviderLane from "../components/ProviderLane.vue";
import RefreshIcon from "../components/RefreshIcon.vue";
import { navigateMain } from "../features/app/navigation";
import { useQuotaStore } from "../features/quota/store";
import { presentOverall } from "../lib/status";

const { t } = useI18n();
const router = useRouter();
const quota = useQuotaStore();

const liveMessage = computed(() => {
  const leader = presentOverall(quota.ordered);
  return leader ? t(leader.presentation.titleKey) : "";
});

const isDev = import.meta.env.DEV;

function openSettings(): void {
  void navigateMain(router, "settings", "settings-title", "quota");
}
</script>

<template>
  <main class="main">
    <p class="visually-hidden" aria-live="polite">{{ liveMessage }}</p>

    <div class="main__inner">
      <header class="main__header">
        <OverallSignal
          :providers="quota.ordered"
          variant="full"
          title-id="main-quota-title"
          :title-tabindex="-1"
        >
          <template #actions>
            <button
              id="main-settings-trigger"
              type="button"
              class="button button--flat button--sm"
              @click="openSettings"
            >
              {{ t("common.settings") }}
            </button>
            <button
              type="button"
              class="button button--primary button--sm"
              @click="quota.refresh()"
            >
              <RefreshIcon :spinning="quota.busy" />
              {{ t("common.refresh") }}
            </button>
          </template>
        </OverallSignal>
      </header>

      <section class="main__lanes" :aria-label="t('a11y.statusRegion')">
        <ProviderLane
          v-for="provider in quota.ordered"
          :key="provider.provider"
          :provider="provider"
          variant="full"
        />
      </section>

      <DevScenarioBar v-if="isDev" />
    </div>
  </main>
</template>

<style scoped>
.main {
  min-block-size: 100vh;
  padding: clamp(1.5rem, 4vw, 2.5rem);
  background: var(--surface-primary);
}

.main__inner {
  display: grid;
  gap: var(--space-5);
  /* 单列堆叠：进度条需要横向空间，两列会把轨道挤成短线 */
  max-inline-size: 45rem;
  margin-inline: auto;
}

.main__lanes {
  display: grid;
  gap: var(--space-5);
}

@media (max-width: 640px) {
  .main__inner {
    gap: var(--space-4);
  }
}
</style>
