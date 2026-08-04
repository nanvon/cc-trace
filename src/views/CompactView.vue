<script setup lang="ts">
/**
 * 紧凑额度面板：回答「现在额度紧不紧、今日／本周 API 等值费用是多少」。
 *
 * 只承载总体状态、两个 Provider 额度与近期费用概览、刷新、主窗口入口、设置与退出。
 * Token 明细、完整错误诊断、历史、筛选与设置表单都不在这里，
 * 见 `docs/信息架构与核心流程.md` 第 5 节。
 *
 * 四个操作在头部以图标按钮呈现（ADR-0017）：380px 宽度容不下四个文字按钮。
 * 每个按钮都有 Tooltip 与可访问名称，不依赖图标猜测。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { useAppShell } from "../app/shell";
import OverallSignal from "../components/OverallSignal.vue";
import ProviderLane from "../components/ProviderLane.vue";
import RefreshIcon from "../components/RefreshIcon.vue";
import {
  openMainWindow,
  openSettingsWindow,
  quitApp,
  setCompactHeight,
} from "../features/app/windows";
import { useUsageStore } from "../features/usage/store";
import { presentOverall } from "../lib/status";

const { t } = useI18n();
const usage = useUsageStore();
const { quota, settings } = useAppShell("compact");

const COMPACT_TITLE_ID = "compact-title";
const entered = ref(false);

const headerRef = ref<HTMLElement | null>(null);
const lanesInnerRef = ref<HTMLElement | null>(null);

/** 面板从系统区域图标的方向产生：macOS 在图标下方，Windows 在托盘上方。 */
const origin = computed(() => (settings.status?.platform === "macos" ? "top" : "bottom"));

/** 播报总体状态，不逐个 Provider 重复播报无变化的刷新结果。 */
const liveMessage = computed(() => {
  const leader = presentOverall(quota.ordered);
  return leader ? t(leader.presentation.titleKey) : "";
});

/** 用量扫描是独立状态；不能借额度状态点或转圈颜色让辅助技术猜测。 */
const usageLiveMessage = computed(() => {
  if (usage.unavailable) {
    return t("compact.usage.scanUnavailable");
  }
  if (usage.scanning) {
    return t("compact.usage.scanScanning");
  }
  if (usage.partial) {
    return t("compact.usage.scanPartial");
  }
  if (usage.status?.finishedAt) {
    return t("compact.usage.scanComplete");
  }
  return "";
});

/**
 * 面板需要的高度 = 头部 + lane 区内容 + lane 区上下留白。
 *
 * 刻意不量 `.panel`：它是 `100vh`，量到的永远是刚设过去的窗口高度，会形成自指，
 * 高度再也不会变。lane 区的内层容器是自然高度，不受窗口高度约束，量它才有意义——
 * 顺带也让窗口变高不会反过来触发新一轮量测。
 */
function measureContentHeight(): number | null {
  const header = headerRef.value;
  const inner = lanesInnerRef.value;
  const lanes = inner?.parentElement;
  if (!header || !inner || !lanes) {
    return null;
  }

  const style = getComputedStyle(lanes);
  const padding = parseFloat(style.paddingBlockStart) + parseFloat(style.paddingBlockEnd);

  return header.getBoundingClientRect().height + inner.getBoundingClientRect().height + padding;
}

/** 亚像素抖动不值得往返一次 IPC，Rust 侧还有一层同样的阈值。 */
let reportedHeight = 0;

function syncWindowHeight(): void {
  const height = measureContentHeight();
  if (height === null || Math.abs(height - reportedHeight) < 1) {
    return;
  }
  reportedHeight = height;
  void setCompactHeight(height);
}

let contentObserver: ResizeObserver | null = null;
let usagePollTimer: number | null = null;
let panelFocused = false;

function clearUsagePoll(): void {
  if (usagePollTimer !== null) {
    window.clearTimeout(usagePollTimer);
    usagePollTimer = null;
  }
}

function scheduleUsagePoll(): void {
  if (usagePollTimer !== null || !panelFocused) {
    return;
  }
  usagePollTimer = window.setTimeout(async () => {
    usagePollTimer = null;
    await usage.poll();
    syncWindowHeight();
    scheduleUsagePoll();
  }, 1_000);
}

/**
 * 窗口是复用的：隐藏后再次显示不会重新挂载组件，因此入场动效与初始焦点
 * 都绑定在窗口的 focus 上。
 */
function handleWindowFocus(): void {
  panelFocused = true;
  entered.value = false;
  requestAnimationFrame(() => {
    entered.value = true;
  });
  document.getElementById(COMPACT_TITLE_ID)?.focus({ preventScroll: true });
  syncWindowHeight();
  void usage.load().then(syncWindowHeight);
  scheduleUsagePoll();
}

function handleWindowBlur(): void {
  panelFocused = false;
  clearUsagePoll();
}

onMounted(() => {
  document.body.dataset.surface = "compact";
  window.addEventListener("focus", handleWindowFocus);
  window.addEventListener("blur", handleWindowBlur);

  // 初次挂载可能发生在隐藏窗口，不把应用启动误当成用户打开；这里只准备视觉与焦点。
  entered.value = true;
  syncWindowHeight();

  void usage.load().then(() => {
    syncWindowHeight();
  });

  // 若 webview 是在已可见、已聚焦时才挂载，focus 事件可能早于监听器。
  void getCurrentWindow()
    .isFocused()
    .then((focused) => {
      if (focused) {
        handleWindowFocus();
      }
    })
    .catch(() => {
      // 浏览器预览没有 Tauri 窗口桥；已有空态仍可用于静态检查。
    });

  // 隐藏期间 webview 没有销毁，额度变化照样会触发量测，因此下次显示出来的
  // 就已经是正确的高度，不会看到「先按旧高度出现再跳一下」。
  contentObserver = new ResizeObserver(syncWindowHeight);
  if (headerRef.value) {
    contentObserver.observe(headerRef.value);
  }
  if (lanesInnerRef.value) {
    contentObserver.observe(lanesInnerRef.value);
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("focus", handleWindowFocus);
  window.removeEventListener("blur", handleWindowBlur);
  clearUsagePoll();
  contentObserver?.disconnect();
  contentObserver = null;
});
</script>

<template>
  <main class="panel" :class="{ 'panel--entered': entered }" :data-origin="origin">
    <p class="visually-hidden" aria-live="polite">{{ liveMessage }}</p>
    <p class="visually-hidden" aria-live="polite">{{ usageLiveMessage }}</p>

    <header ref="headerRef" class="panel__header">
      <OverallSignal
        :providers="quota.ordered"
        variant="compact"
        :title-id="COMPACT_TITLE_ID"
        :title-tabindex="-1"
      >
        <template #actions>
          <button
            type="button"
            class="button button--quiet button--icon button--sm"
            :aria-label="t('a11y.refreshAll')"
            :title="t('common.refresh')"
            @click="quota.refresh()"
          >
            <RefreshIcon :spinning="quota.busy" />
          </button>

          <button
            type="button"
            class="button button--quiet button--icon button--sm"
            :aria-label="t('common.details')"
            :title="t('common.details')"
            @click="openMainWindow"
          >
            <svg
              viewBox="0 0 16 16"
              width="16"
              height="16"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              aria-hidden="true"
              focusable="false"
            >
              <path d="M2.5 13.5V9M6.5 13.5V4M10.5 13.5V6.5M14 13.5V2.5" />
            </svg>
          </button>

          <button
            type="button"
            class="button button--quiet button--icon button--sm"
            :aria-label="t('common.settings')"
            :title="t('common.settings')"
            @click="openSettingsWindow"
          >
            <svg
              viewBox="0 0 16 16"
              width="16"
              height="16"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
              focusable="false"
            >
              <path d="M2 4.5h5M11 4.5h3M2 11.5h3M9 11.5h5" />
              <circle cx="9" cy="4.5" r="1.75" />
              <circle cx="7" cy="11.5" r="1.75" />
            </svg>
          </button>

          <button
            type="button"
            class="button button--quiet button--icon button--sm"
            :aria-label="t('common.quit')"
            :title="t('common.quit')"
            @click="quitApp"
          >
            <svg
              viewBox="0 0 16 16"
              width="16"
              height="16"
              fill="none"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
              aria-hidden="true"
              focusable="false"
            >
              <path d="M8 2v6" />
              <path d="M4.6 4.4a5 5 0 1 0 6.8 0" />
            </svg>
          </button>
        </template>
      </OverallSignal>
    </header>

    <section class="panel__lanes" :aria-label="t('a11y.statusRegion')">
      <!-- 内层容器承担 lane 之间的排布，它的自然高度就是窗口该有的内容高度 -->
      <div ref="lanesInnerRef" class="panel__lanes-inner">
        <ProviderLane
          v-for="provider in quota.ordered"
          :key="provider.provider"
          :provider="provider"
          variant="compact"
          :usage-costs="usage.costs[provider.provider]"
          :usage-scanning="usage.loading"
        />
      </div>
    </section>
  </main>
</template>

<style scoped>
.panel {
  display: grid;
  grid-template-rows: auto 1fr;
  block-size: 100vh;
  /* 平台材质属于窗口外壳；内容层保持实色以保证对比度可靠 */
  background: var(--surface-primary);
  border: 1px solid var(--ring-panel);
  border-radius: var(--radius-shell);
  overflow: hidden;
}

/* 分隔线属于头部：lane 区滚动时它留在原地 */
.panel__header {
  padding: 0.75rem 0.75rem 0.75rem 1rem;
  border-block-end: 1px solid var(--border-subtle);
}

/* 留白留在外层，排布交给内层：内层高度因此只反映内容，不含窗口的富余空间 */
.panel__lanes {
  padding: 0.75rem;
  overflow-y: auto;
}

.panel__lanes-inner {
  display: grid;
  align-content: start;
  gap: 0.625rem;
}

@media (prefers-reduced-motion: no-preference) {
  .panel {
    opacity: 0;
    scale: 0.985;
    transition:
      opacity var(--motion-panel) var(--ease-out),
      scale var(--motion-panel) var(--ease-out);
  }

  .panel[data-origin="top"] {
    transform-origin: top center;
  }

  .panel[data-origin="bottom"] {
    transform-origin: bottom center;
  }

  .panel--entered {
    opacity: 1;
    scale: 1;
  }
}
</style>
