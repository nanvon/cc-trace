<script setup lang="ts">
/**
 * 主窗口左侧分组侧边栏（ADR-0024）。
 *
 * 视图组（用量／对话／时间线）＋数据源组（全部／各源）＋设置钉底。
 * 数据源选中是全局内存态，跨页面共享、重启回到「全部」；进入设置视图时隐藏数据源组。
 * 视图切换由 Vue Router 承担，焦点目标与原生导航见 `features/app/navigation.ts`。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";

import { navigateMain } from "../features/app/navigation";
import type { MainFocusTarget } from "../features/app/navigation";
import type { UsageSource } from "../features/usage/contracts";
import { useUsageStore } from "../features/usage/store";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const usage = useUsageStore();

const isSettings = computed(() => route.name === "settings");

const VIEW_FOCUS: Record<"main" | "conversations" | "timeline", MainFocusTarget> = {
  main: "usage-title",
  conversations: "conversations-title",
  timeline: "timeline-title",
};

const views = computed(() => [
  { name: "main" as const, label: t("main.title"), key: "usage" },
  { name: "conversations" as const, label: t("conversations.title"), key: "conversations" },
  { name: "timeline" as const, label: t("timeline.title"), key: "timeline" },
]);

const currentView = computed(() => {
  if (route.name === "settings") return "settings";
  if (route.name === "conversation-detail") return "conversations";
  return views.value.find((view) => view.name === route.name)?.key ?? "usage";
});

function goView(name: "main" | "conversations" | "timeline"): void {
  const target = name === "main" ? "quota" : name;
  void navigateMain(router, target, VIEW_FOCUS[name]);
}

function goSettings(): void {
  void navigateMain(router, "settings", "settings-title");
}

function selectSource(source: "all" | UsageSource): void {
  usage.selectSource(source);
  if (isSettings.value) return;
  void usage.loadDashboard(usage.dashboardRange);
}

const sources = computed(() => usage.sourceFilterOptions);
</script>

<template>
  <aside class="mw-sidebar" :aria-label="t('a11y.sidebar')">
    <nav class="sb-group" :aria-label="t('sidebar.views')">
      <h3>{{ t("sidebar.views") }}</h3>
      <button
        v-for="view in views"
        :key="view.name"
        type="button"
        class="sb-item"
        :class="{ on: currentView === view.key }"
        :aria-current="currentView === view.key ? 'page' : undefined"
        @click="goView(view.name)"
      >
        {{ view.label }}
      </button>
    </nav>

    <nav v-if="!isSettings" class="sb-group" :aria-label="t('sidebar.sources')">
      <h3>{{ t("sidebar.sources") }}</h3>
      <button
        v-for="source in sources"
        :key="source"
        type="button"
        class="sb-item"
        :class="{ on: usage.sourceFilter === source }"
        :aria-pressed="usage.sourceFilter === source"
        @click="selectSource(source)"
      >
        <i v-if="source !== 'all'" class="sb-dot" :data-provider="source" aria-hidden="true"></i>
        {{ source === "all" ? t("sidebar.allSources") : t(`provider.${source}`) }}
      </button>
    </nav>

    <div class="sb-spacer"></div>

    <div class="sb-bottom">
      <div class="sb-group">
        <button
          type="button"
          class="sb-item sb-item--settings"
          :class="{ on: currentView === 'settings' }"
          :aria-current="currentView === 'settings' ? 'page' : undefined"
          @click="goSettings"
        >
          {{ t("common.settings") }}
        </button>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.mw-sidebar {
  inline-size: 11rem;
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 0.875rem 0.625rem 0.75rem;
  border-inline-end: 1px solid var(--border-subtle);
  background: color-mix(in srgb, var(--surface-primary) 88%, var(--track-background) 12%);
}

.sb-group {
  display: grid;
  gap: 0.125rem;
}

.sb-group > h3 {
  margin: 0 0 0.25rem 0.625rem;
  color: var(--text-secondary);
  font-size: 0.65625rem;
  font-weight: 680;
  letter-spacing: 0.05em;
}

.sb-item {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-block-size: 1.875rem;
  padding: 0.3125rem 0.625rem;
  border: 0;
  border-radius: 0.5rem;
  color: var(--text-secondary);
  background: transparent;
  font-size: 0.78125rem;
  font-weight: 600;
  text-align: start;
  white-space: nowrap;
}

.sb-item:hover {
  color: var(--text-primary);
  background: color-mix(in srgb, var(--text-primary) 7%, transparent);
}

.sb-item.on {
  color: var(--text-primary);
  background: var(--surface-raised);
  box-shadow: 0 1px 2px rgb(24 24 27 / 10%);
  font-weight: 650;
}

.sb-dot {
  inline-size: 0.4375rem;
  block-size: 0.4375rem;
  flex: none;
  border-radius: 0.15625rem;
  background: var(--cat-codex);
}

.sb-dot[data-provider="claude"] {
  background: var(--cat-claude);
}

.sb-dot[data-provider="pi"] {
  background: var(--cat-pi);
}

.sb-dot[data-provider="opencode"] {
  background: var(--cat-opencode);
}

.sb-spacer {
  flex: 1;
}

.sb-bottom {
  padding-block-start: 0.375rem;
  border-block-start: 1px solid var(--border-subtle);
}

.sb-item--settings {
  font-weight: 600;
}

@media (prefers-reduced-motion: no-preference) {
  .sb-item {
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out);
  }
}
</style>
