<script setup lang="ts">
import ProviderRail from "../components/ProviderRail.vue";
import {
  openMainWindow,
  openSettingsWindow,
  quitApp,
} from "../features/app/desktop";
import { previewProviders, usePreviewRefresh } from "../features/quota/preview";

const { isRefreshing, refresh } = usePreviewRefresh();
</script>

<template>
  <main class="compact-shell" :aria-busy="isRefreshing">
    <header class="compact-shell__header">
      <div>
        <p class="utility-label">CC TRACE</p>
        <h1>{{ isRefreshing ? "正在刷新额度…" : "额度状态正常" }}</h1>
        <p>{{ isRefreshing ? "保留上一份有效快照" : "刚刚刷新 · 桌面壳假数据" }}</p>
      </div>
      <button
        class="icon-button"
        type="button"
        aria-label="刷新全部额度"
        @click="refresh"
      >
        ↻
      </button>
    </header>

    <section class="compact-shell__providers" aria-live="polite">
      <ProviderRail
        v-for="provider in previewProviders"
        :key="provider.id"
        :provider="provider"
        compact
      />
    </section>

    <footer class="compact-shell__actions">
      <button class="primary-button" type="button" @click="openMainWindow">
        查看详情
      </button>
      <button class="quiet-button" type="button" @click="openSettingsWindow">
        设置
      </button>
      <button class="quiet-button" type="button" @click="quitApp">退出</button>
    </footer>
  </main>
</template>

<style scoped>
.compact-shell {
  min-height: 100vh;
  padding: var(--space-4);
  background: var(--surface-primary);
}

.compact-shell__header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--space-4);
}

h1 {
  margin: 0;
  font-size: 1.25rem;
  letter-spacing: -0.02em;
}

.compact-shell__header p:last-child {
  margin: var(--space-1) 0 0;
  color: var(--text-secondary);
  font-size: 0.8rem;
}

.compact-shell__providers {
  display: grid;
  gap: var(--space-3);
  margin: var(--space-4) 0;
}

.compact-shell__actions {
  display: flex;
  gap: var(--space-2);
}
</style>
