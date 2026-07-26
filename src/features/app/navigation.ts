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

export type MainNavigationTarget = "quota" | "settings";
export type MainFocusTarget = "quota-title" | "settings-title" | "settings-trigger";

export function isMainNavigationTarget(value: unknown): value is MainNavigationTarget {
  return value === "quota" || value === "settings";
}

export function mainRoute(
  target: MainNavigationTarget,
  origin?: "quota",
): RouteLocationRaw {
  if (target === "settings") {
    return {
      name: "settings",
      query: origin ? { origin } : undefined,
    };
  }
  return { name: "main" };
}

export async function navigateMain(
  router: Router,
  target: MainNavigationTarget,
  focusTarget: MainFocusTarget,
  origin?: "quota",
): Promise<void> {
  await router.replace(mainRoute(target, origin));
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
