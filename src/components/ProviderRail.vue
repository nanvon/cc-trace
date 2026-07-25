<script setup lang="ts">
import type { PreviewProvider } from "../features/quota/preview";

defineProps<{
  provider: PreviewProvider;
  compact?: boolean;
}>();
</script>

<template>
  <article class="provider-rail">
    <header>
      <strong>{{ provider.name }}</strong>
      <span>假数据</span>
    </header>

    <div class="provider-rail__value">
      <span>{{ provider.remaining }}%</span>
      <small>{{ provider.reset }}重置</small>
    </div>

    <div
      class="provider-rail__track"
      role="progressbar"
      :aria-label="`${provider.name} 剩余额度`"
      aria-valuemin="0"
      aria-valuemax="100"
      :aria-valuenow="provider.remaining"
      :aria-valuetext="`剩余 ${provider.remaining}%`"
    >
      <span :style="{ width: `${provider.remaining}%` }"></span>
    </div>

    <footer>
      <span>{{ provider.window }}</span>
      <span>{{ compact ? "本地壳预览" : "等待 Provider 接入" }}</span>
    </footer>
  </article>
</template>

<style scoped>
.provider-rail {
  padding: var(--space-4);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
}

.provider-rail header,
.provider-rail__value,
.provider-rail footer {
  display: flex;
  justify-content: space-between;
  gap: var(--space-3);
}

.provider-rail header {
  align-items: baseline;
}

.provider-rail header strong {
  min-width: 0;
  overflow: hidden;
  font-size: 1rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.provider-rail header span,
.provider-rail footer,
.provider-rail__value small {
  color: var(--text-secondary);
  font-size: 0.75rem;
}

.provider-rail__value {
  align-items: baseline;
  margin-top: var(--space-4);
}

.provider-rail__value > span {
  font-family: var(--font-data);
  font-size: 1.55rem;
  font-variant-numeric: tabular-nums;
  font-weight: 720;
}

.provider-rail__track {
  height: 0.45rem;
  margin-top: var(--space-2);
  overflow: hidden;
  background: var(--track-background);
  border-radius: 999px;
}

.provider-rail__track > span {
  display: block;
  height: 100%;
  background: var(--text-primary);
  border-radius: inherit;
}

.provider-rail footer {
  margin-top: var(--space-2);
}
</style>
