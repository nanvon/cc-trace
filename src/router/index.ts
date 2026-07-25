import { createRouter, createWebHashHistory } from "vue-router";

import HomeView from "../views/HomeView.vue";
import CompactView from "../views/CompactView.vue";
import SettingsView from "../views/SettingsView.vue";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      name: "home",
      component: HomeView,
    },
    {
      path: "/compact",
      name: "compact",
      component: CompactView,
    },
    {
      path: "/settings",
      name: "settings",
      component: SettingsView,
    },
  ],
});
