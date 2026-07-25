import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { onBeforeUnmount, onMounted, ref } from "vue";

export interface PreviewProvider {
  id: "codex" | "claude";
  name: string;
  remaining: number;
  reset: string;
  window: string;
}

export const previewProviders: PreviewProvider[] = [
  {
    id: "codex",
    name: "Codex",
    remaining: 73,
    reset: "4 小时 12 分后",
    window: "当前窗口",
  },
  {
    id: "claude",
    name: "Claude Code",
    remaining: 38,
    reset: "周一 08:00",
    window: "当前窗口",
  },
];

export function usePreviewRefresh() {
  const isRefreshing = ref(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let unlisten: UnlistenFn | undefined;

  function refresh() {
    isRefreshing.value = true;
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      isRefreshing.value = false;
    }, 900);
  }

  onMounted(async () => {
    try {
      unlisten = await listen("shell://refresh-preview", refresh);
    } catch {
      // Browser preview has no Tauri event bridge; the local refresh button still works.
    }
  });

  onBeforeUnmount(() => {
    window.clearTimeout(timer);
    unlisten?.();
  });

  return {
    isRefreshing,
    refresh,
  };
}
