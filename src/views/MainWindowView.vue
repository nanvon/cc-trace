<script setup lang="ts">
/**
 * 主窗口的持久外壳。
 *
 * 本地用量页与设置共用这一份状态订阅、键盘处理和原生窗口；子路由只替换内容区。
 */
import type { UnlistenFn } from "@tauri-apps/api/event";
import { onBeforeUnmount, onMounted } from "vue";
import { RouterView, useRouter } from "vue-router";

import { useAppShell } from "../app/shell";
import {
  navigateMain,
  onMainNavigation,
  type MainNavigationTarget,
} from "../features/app/navigation";

useAppShell("main");

const router = useRouter();
let unlistenNavigation: UnlistenFn | undefined;

function handleNavigation(target: MainNavigationTarget): void {
  const focusTarget = target === "settings" ? "settings-title" : "usage-title";
  void navigateMain(router, target, focusTarget);
}

function makeLeavingViewInert(element: Element): void {
  element.setAttribute("inert", "");
}

onMounted(async () => {
  try {
    unlistenNavigation = await onMainNavigation(handleNavigation);
  } catch {
    // 纯浏览器预览没有 Tauri 事件桥；主窗口内部导航仍然可用。
  }
});

onBeforeUnmount(() => {
  unlistenNavigation?.();
});
</script>

<template>
  <div class="main-window">
    <RouterView v-slot="{ Component, route }">
      <Transition name="main-view" @before-leave="makeLeavingViewInert">
        <component :is="Component" :key="route.name" />
      </Transition>
    </RouterView>
  </div>
</template>

<style scoped>
.main-window {
  display: grid;
  min-block-size: 100vh;
  overflow-x: hidden;
  background: var(--surface-primary);
}

.main-window > * {
  grid-area: 1 / 1;
}

@media (prefers-reduced-motion: no-preference) {
  .main-view-enter-active {
    transition: opacity var(--motion-base) var(--ease-out);
  }

  .main-view-leave-active {
    pointer-events: none;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .main-view-enter-from,
  .main-view-leave-to {
    opacity: 0;
  }
}
</style>
