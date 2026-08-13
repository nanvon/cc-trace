import { createRouter, createWebHashHistory } from "vue-router";

import MainWindowView from "../views/MainWindowView.vue";

// 页面组件按路由动态导入：compact/onboarding WebView 不解析主窗口图表、日期选择器与
// 对话页代码；chunk 全部来自本地 dist，不引入网络依赖。
const MainView = () => import("../views/MainView.vue");
const ConversationsView = () => import("../views/ConversationsView.vue");
const SettingsView = () => import("../views/SettingsView.vue");
const TimelineView = () => import("../views/TimelineView.vue");
const CompactView = () => import("../views/CompactView.vue");
const OnboardingView = () => import("../views/OnboardingView.vue");

/**
 * 紧凑面板与首次启动仍各自通过 hash 载入独立表面。
 * 本地用量页与设置则是 `main` WebviewWindow 内的两个子路由。
 */
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      component: MainWindowView,
      children: [
        { path: "", name: "main", component: MainView },
        { path: "settings", name: "settings", component: SettingsView },
        { path: "timeline", name: "timeline", component: TimelineView },
        { path: "conversations", name: "conversations", component: ConversationsView },
      ],
    },
    { path: "/compact", name: "compact", component: CompactView },
    { path: "/onboarding", name: "onboarding", component: OnboardingView },
  ],
});
