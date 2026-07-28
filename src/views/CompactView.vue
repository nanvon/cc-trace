<script setup lang="ts">
/**
 * 紧凑额度面板：回答「现在额度紧不紧」。
 *
 * 只承载总体状态、两个 Provider 概览、刷新、主窗口入口、设置与退出。
 * 完整错误诊断、历史与设置表单都不在这里，见 `docs/信息架构与核心流程.md` 第 5 节。
 *
 * 四个操作在头部以图标按钮呈现（ADR-0017）：380px 宽度容不下四个文字按钮。
 * 每个按钮都有 Tooltip 与可访问名称，不依赖图标猜测。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

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
import { presentOverall } from "../lib/status";

const { t } = useI18n();
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

/**
 * 窗口是复用的：隐藏后再次显示不会重新挂载组件，因此入场动效与初始焦点
 * 都绑定在窗口的 focus 上。
 */
function handleWindowFocus(): void {
  entered.value = false;
  requestAnimationFrame(() => {
    entered.value = true;
  });
  document.getElementById(COMPACT_TITLE_ID)?.focus({ preventScroll: true });
  syncWindowHeight();
}

onMounted(() => {
  document.body.dataset.surface = "compact";
  window.addEventListener("focus", handleWindowFocus);
  handleWindowFocus();

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
  contentObserver?.disconnect();
  contentObserver = null;
});
</script>

<template>
  <main class="panel" :class="{ 'panel--entered': entered }" :data-origin="origin">
    <p class="visually-hidden" aria-live="polite">{{ liveMessage }}</p>

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
              stroke-width="1.4"
              stroke-linecap="round"
              aria-hidden="true"
              focusable="false"
            >
              <circle cx="8" cy="8" r="2.2" />
              <path
                d="M8 1.6v1.9M8 12.5v1.9M14.4 8h-1.9M3.5 8H1.6M12.5 3.5l-1.3 1.3M4.8 11.2l-1.3 1.3M12.5 12.5l-1.3-1.3M4.8 4.8L3.5 3.5"
              />
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
  padding: var(--space-3) 0.8125rem;
  border-block-end: 1px solid var(--border-subtle);
}

/* 留白留在外层，排布交给内层：内层高度因此只反映内容，不含窗口的富余空间 */
.panel__lanes {
  padding: 0.5625rem;
  overflow-y: auto;
}

.panel__lanes-inner {
  display: grid;
  align-content: start;
  gap: 0.5625rem;
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
