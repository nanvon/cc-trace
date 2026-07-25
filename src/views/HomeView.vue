<script setup lang="ts">
import ProviderRail from "../components/ProviderRail.vue";
import { openSettingsWindow } from "../features/app/desktop";
import { previewProviders, usePreviewRefresh } from "../features/quota/preview";

const { isRefreshing, refresh } = usePreviewRefresh();
</script>

<template>
  <main class="main-shell">
    <header class="main-shell__header">
      <div>
        <p class="utility-label">桌面壳 · 假数据</p>
        <h1>额度状态正常</h1>
        <p class="supporting">
          当前窗口只用于验证 Tray、窗口关系和假数据展示。
        </p>
      </div>

      <div class="main-shell__actions">
        <button class="quiet-button" type="button" @click="openSettingsWindow">
          设置
        </button>
        <button class="primary-button" type="button" @click="refresh">
          {{ isRefreshing ? "刷新中…" : "刷新全部" }}
        </button>
      </div>
    </header>

    <section class="provider-grid" aria-label="Provider 额度预览">
      <ProviderRail
        v-for="provider in previewProviders"
        :key="provider.id"
        :provider="provider"
      />
    </section>

    <section class="trust-note" aria-labelledby="trust-title">
      <div>
        <p class="utility-label">数据是否可信</p>
        <h2 id="trust-title">这是桌面壳合成快照</h2>
      </div>
      <p>
        当前数值不来自 Codex 或 Claude Code。双平台 Tray
        验证完成后，才会开始第一个 Provider 最小闭环。
      </p>
    </section>
  </main>
</template>

<style scoped>
.main-shell {
  min-height: 100%;
  padding: clamp(2rem, 5vw, 4.5rem);
  background: var(--surface-primary);
}

.main-shell__header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--space-6);
  max-width: 72rem;
  margin: 0 auto;
}

h1 {
  margin: 0;
  font-size: clamp(2rem, 5vw, 3.7rem);
  letter-spacing: -0.045em;
  line-height: 1;
}

.supporting {
  max-width: 36rem;
  margin: var(--space-3) 0 0;
  color: var(--text-secondary);
  line-height: 1.65;
}

.main-shell__actions {
  display: flex;
  gap: var(--space-2);
}

.provider-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
  max-width: 72rem;
  margin: var(--space-8) auto 0;
}

.trust-note {
  display: grid;
  grid-template-columns: minmax(13rem, 0.55fr) minmax(0, 1fr);
  gap: var(--space-8);
  max-width: 72rem;
  margin: var(--space-8) auto 0;
  padding-top: var(--space-6);
  border-top: 1px solid var(--border-subtle);
}

.trust-note h2,
.trust-note p {
  margin: 0;
}

.trust-note h2 {
  font-size: 1.15rem;
}

.trust-note > p {
  color: var(--text-secondary);
  line-height: 1.7;
}

@media (max-width: 720px) {
  .main-shell__header,
  .trust-note {
    grid-template-columns: 1fr;
    flex-direction: column;
  }

  .provider-grid {
    grid-template-columns: 1fr;
  }
}
</style>
