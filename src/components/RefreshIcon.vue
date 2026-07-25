<script setup lang="ts">
/**
 * 刷新图标。只在真实刷新任务进行时旋转，请求结束立即进入结果状态。
 * Reduced motion 下改为静态：状态由按钮旁的文案与 `aria-live` 承担。
 */
withDefaults(defineProps<{ spinning?: boolean }>(), { spinning: false });
</script>

<template>
  <svg
    class="refresh-icon"
    :class="{ 'refresh-icon--spinning': spinning }"
    viewBox="0 0 16 16"
    width="16"
    height="16"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
  >
    <path d="M13.4 8a5.4 5.4 0 1 1-1.7-3.95" />
    <path d="M13.6 2.2v2.9h-2.9" />
  </svg>
</template>

<style scoped>
.refresh-icon {
  display: block;
  transform-box: fill-box;
  transform-origin: center;
}

@media (prefers-reduced-motion: no-preference) {
  .refresh-icon--spinning {
    animation: refresh-spin 900ms linear infinite;
  }
}

@keyframes refresh-spin {
  to {
    rotate: 360deg;
  }
}
</style>
