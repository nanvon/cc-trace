import { defineStore } from "pinia";
import { ref } from "vue";

import { getAppStatus, type AppStatus } from "../features/app/api";

export const useAppStore = defineStore("app", () => {
  const status = ref<AppStatus | null>(null);
  const statusError = ref<string | null>(null);

  async function loadStatus() {
    try {
      status.value = await getAppStatus();
      statusError.value = null;
    } catch {
      statusError.value = "TAURI_BRIDGE_UNAVAILABLE";
    }
  }

  return {
    status,
    statusError,
    loadStatus,
  };
});
