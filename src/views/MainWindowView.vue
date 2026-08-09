<script setup lang="ts">
/**
 * 主窗口的持久外壳。
 *
 * 本地用量页、Timeline、Conversations 与设置共用这一份状态订阅、键盘处理和原生窗口；
 * 左侧分组侧边栏承担视图切换与数据源过滤（ADR-0024），子路由只替换内容区。
 */
import type { UnlistenFn } from "@tauri-apps/api/event";
import { onBeforeUnmount, onMounted } from "vue";
import { RouterView, useRouter } from "vue-router";

import MainSidebar from "../components/MainSidebar.vue";
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
  const focusTarget =
    target === "settings"
      ? "settings-title"
      : target === "timeline"
        ? "timeline-title"
        : target === "conversations"
          ? "conversations-title"
          : "usage-title";
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
    <MainSidebar />
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
  grid-template-columns: auto minmax(0, 1fr);
  block-size: 100vh;
  overflow: hidden;
  background: var(--surface-primary);
}

.main-window > .mw-sidebar {
  grid-column: 1;
}

.main-window > :not(.mw-sidebar) {
  grid-area: 1 / 2;
  min-inline-size: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
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
