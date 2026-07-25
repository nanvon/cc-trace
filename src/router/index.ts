import { createRouter, createWebHashHistory } from "vue-router";

import CompactView from "../views/CompactView.vue";
import MainView from "../views/MainView.vue";
import OnboardingView from "../views/OnboardingView.vue";
import SettingsView from "../views/SettingsView.vue";

/**
 * 每个 Tauri 窗口通过 hash 载入自己的表面，见 `src-tauri/tauri.conf.json`。
 * 这里没有跨窗口导航：窗口之间的关系由 Rust 平台层管理。
 */
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "main", component: MainView },
    { path: "/compact", name: "compact", component: CompactView },
    { path: "/settings", name: "settings", component: SettingsView },
    { path: "/onboarding", name: "onboarding", component: OnboardingView },
  ],
});
