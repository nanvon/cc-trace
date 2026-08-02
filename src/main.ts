import { createPinia } from "pinia";
import { createApp } from "vue";
import { setupCalendar } from "v-calendar";

import App from "./App.vue";
import { i18n } from "./i18n";
import { router } from "./router";
import "./styles/tokens.css";
import "./styles/base.css";

createApp(App).use(createPinia()).use(router).use(i18n).use(setupCalendar).mount("#app");
