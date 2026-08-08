/**
 * 主窗口的一次性导航意图。
 *
 * Rust 只把目标发给 `main` Webview；这里负责把目标映射为 Vue Router 路由。
 * 它不是业务状态，不进入 Pinia、设置文件或额度缓存。
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { nextTick } from "vue";
import type { RouteLocationRaw, Router } from "vue-router";

export const EVENT_MAIN_NAVIGATION = "navigation://main";

export type MainNavigationTarget = "quota" | "settings" | "timeline" | "conversations";
export type MainFocusTarget =
  "usage-title" | "settings-title" | "timeline-title" | "conversations-title";

export function isMainNavigationTarget(value: unknown): value is MainNavigationTarget {
  return (
    value === "quota" || value === "settings" || value === "timeline" || value === "conversations"
  );
}

export function mainRoute(target: MainNavigationTarget): RouteLocationRaw {
  if (target === "settings") {
    return { name: "settings" };
  }
  if (target === "timeline") {
    return { name: "timeline" };
  }
  if (target === "conversations") {
    return { name: "conversations" };
  }
  return { name: "main" };
}

export async function navigateMain(
  router: Router,
  target: MainNavigationTarget,
  focusTarget: MainFocusTarget,
): Promise<void> {
  await router.replace(mainRoute(target));
  await nextTick();
  document.getElementById(`main-${focusTarget}`)?.focus({ preventScroll: true });
}

export function onMainNavigation(
  handler: (target: MainNavigationTarget) => void,
): Promise<UnlistenFn> {
  return listen<unknown>(EVENT_MAIN_NAVIGATION, (event) => {
    if (isMainNavigationTarget(event.payload)) {
      handler(event.payload);
    }
  });
}
