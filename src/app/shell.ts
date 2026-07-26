/**
 * 每个窗口共用的启动流程：读取状态、订阅事件、绑定键盘。
 *
 * 三个窗口各自是独立的 webview，因此每个窗口都要自己订阅一次。主窗口的额度与设置
 * 子路由共用一份订阅；所有窗口消费的是同一份 Rust 状态，不存在第二个状态源。
 */

import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { onBeforeUnmount, onMounted } from "vue";

import { hideCompactPanel, openSettingsWindow, quitApp } from "../features/app/windows";
import { onQuotaUpdated, onRefreshState } from "../features/quota/api";
import { useQuotaStore } from "../features/quota/store";
import { onSettingsUpdated } from "../features/settings/api";
import { useSettingsStore } from "../features/settings/store";

export type Surface = "compact" | "main" | "onboarding";

export function useAppShell(surface: Surface) {
  const quota = useQuotaStore();
  const settings = useSettingsStore();
  const subscriptions: UnlistenFn[] = [];

  /**
   * `⌘W` / `Ctrl+W` 关闭当前表面。紧凑面板必须走 Rust 命令：它同时记录隐藏时刻，
   * 供系统区域图标的主点击去抖使用。
   */
  async function closeSurface(): Promise<void> {
    if (surface === "compact") {
      await hideCompactPanel();
      return;
    }
    try {
      await getCurrentWindow().hide();
    } catch {
      // 窗口已经不可用时无需补救：系统区域入口仍然可用。
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) {
      return;
    }

    // Esc 把控制权交回系统区域入口，见 docs/信息架构与核心流程.md 第 4.1 节。
    if (event.key === "Escape" && surface === "compact") {
      event.preventDefault();
      void hideCompactPanel();
      return;
    }

    // macOS 用 ⌘，Windows 用 Ctrl。同时接受两者，业务代码因此不需要平台判断。
    if (!event.metaKey && !event.ctrlKey) {
      return;
    }

    switch (event.key.toLowerCase()) {
      case "r":
        event.preventDefault();
        void quota.refresh();
        break;
      case ",":
        event.preventDefault();
        void openSettingsWindow();
        break;
      case "w":
        event.preventDefault();
        void closeSurface();
        break;
      case "q":
        event.preventDefault();
        void quitApp();
        break;
      default:
        break;
    }
  }

  onMounted(async () => {
    window.addEventListener("keydown", handleKeydown);

    await Promise.allSettled([settings.load(), quota.load()]);

    try {
      subscriptions.push(
        await onQuotaUpdated(quota.adopt),
        await onRefreshState(quota.adoptRefreshState),
        await onSettingsUpdated(settings.adopt),
      );
    } catch {
      // 纯浏览器预览没有 Tauri 事件桥；界面仍可用已加载的状态渲染。
    }
  });

  onBeforeUnmount(() => {
    window.removeEventListener("keydown", handleKeydown);
    subscriptions.forEach((unlisten) => unlisten());
    subscriptions.length = 0;
  });

  return { quota, settings, closeSurface };
}
