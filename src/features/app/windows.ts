/**
 * 窗口操作。全部经过窄 Tauri command，前端没有任意窗口创建能力。
 *
 * 每个调用都吞掉失败：窗口打不开时不该让界面抛出未处理的 Promise 拒绝，
 * 用户能看到的补救路径仍在系统区域图标上。
 */

import { invoke } from "@tauri-apps/api/core";

async function call(command: string): Promise<void> {
  try {
    await invoke(command);
  } catch {
    // 窗口不可用是终端状态，没有可展示的下一步；系统区域入口仍然可用。
  }
}

/**
 * 上报紧凑面板内容需要的高度，单位是 CSS 像素。
 *
 * 只是「需要多高」的量测结果：收进允许区间、决定要不要连带重新锚定都在 Rust 平台层。
 */
export async function setCompactHeight(contentHeight: number): Promise<void> {
  try {
    await invoke("window_set_compact_height", { contentHeight });
  } catch {
    // 纯浏览器预览没有 Tauri 桥；面板在固定视口下仍按 100vh 正确渲染。
  }
}

export const openMainWindow = () => call("window_open_main");
export const openSettingsWindow = () => call("window_open_settings");
export const openOnboardingWindow = () => call("window_open_onboarding");
export const openCompactPanel = () => call("window_open_compact");
export const hideCompactPanel = () => call("window_hide_compact");
export const quitApp = () => call("app_quit");
