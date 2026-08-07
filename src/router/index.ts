import { createRouter, createWebHashHistory } from "vue-router";

import CompactView from "../views/CompactView.vue";
import ConversationDetailView from "../views/ConversationDetailView.vue";
import ConversationsView from "../views/ConversationsView.vue";
import MainView from "../views/MainView.vue";
import MainWindowView from "../views/MainWindowView.vue";
import OnboardingView from "../views/OnboardingView.vue";
import SettingsView from "../views/SettingsView.vue";
import TimelineView from "../views/TimelineView.vue";

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
        {
          path: "conversations/:key",
          name: "conversation-detail",
          component: ConversationDetailView,
        },
      ],
    },
    { path: "/compact", name: "compact", component: CompactView },
    { path: "/onboarding", name: "onboarding", component: OnboardingView },
  ],
});
