<script setup lang="ts">
/**
 * 桌面悬浮窗：把已勾选「悬浮窗」的服务的主要额度常驻在桌面上。
 *
 * 对应 cc-bar 的 `FloatingContentView`：每行是服务标识、进度条与定宽百分比，
 * 颜色按余量分档（`lib/quotaTone.ts`），进度 = 主要额度的剩余百分比。
 * 数据与紧凑面板是同一份额度事件（`AppCore::emit_quota_state`），没有第二个数据源。
 *
 * 窗口本身（置顶、不进任务栏、不抢焦点、位置记忆、屏幕外回落）都在 Rust 平台层；
 * 这里只负责内容、拖动入口与「内容多大」的量测上报。
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import { useAppShell } from "../app/shell";
import { setFloatingSize, showFloatingContextMenu } from "../features/app/windows";
import { primaryWindow, type ProviderId, type ProviderSnapshot } from "../features/quota/contracts";
import { formatPercent } from "../lib/format";
import { providerLabel } from "../lib/labels";
import { displayQuotaTone, type QuotaTone } from "../lib/quotaTone";
import { presentProvider } from "../lib/status";

const { t, locale } = useI18n();
const { quota, settings } = useAppShell("floating");

/** 服务标识的文字形态。不依赖外部 logo 资源，深浅色下都由品牌分类色保证对比。 */
const MARKS: Record<ProviderId, string> = {
  codex: "Cx",
  claude: "Cl",
  antigravity: "Ag",
  cursor: "Cu",
  commandCode: "CC",
};

interface Row {
  key: string;
  provider: ProviderId;
  mark: string;
  /** 进度条填充比例；无数值或无上限时为 null，不画填充。 */
  fill: number | null;
  text: string;
  tone: QuotaTone;
  unlimited: boolean;
  label: string;
}

function buildRow(provider: ProviderSnapshot): Row {
  const window = primaryWindow(provider.snapshot);
  const rail = presentProvider(provider).rail;
  const name = providerLabel(t, provider.provider);
  const base = {
    key: provider.subjectId,
    provider: provider.provider,
    mark: MARKS[provider.provider],
  };

  if (window?.unlimited) {
    return {
      ...base,
      fill: null,
      text: "∞",
      tone: "none",
      unlimited: true,
      label: `${name} ${t("floating.unlimited")}`,
    };
  }

  const remaining =
    window && rail !== "empty" && rail !== "loading" ? window.remainingPercent : null;
  const text = remaining === null ? t("quota.noValue") : formatPercent(locale.value, remaining);
  return {
    ...base,
    fill: remaining === null ? null : Math.max(0, Math.min(100, remaining)),
    text,
    tone: displayQuotaTone(remaining, rail),
    unlimited: false,
    label: `${name} ${remaining === null ? t("a11y.noQuota") : t("a11y.remaining", { percent: text })}`,
  };
}

/**
 * 有效可见 = 额度总开关开启且悬浮窗子开关开启（cc-bar 的 `effectiveFloatingVisibility`）。
 * 每个服务只显示主账号；导入的副账号不进悬浮窗。设置未加载时不显示，避免闪出未勾选的服务。
 */
const rows = computed<Row[]>(() => {
  const services = settings.settings?.services;
  if (!services) {
    return [];
  }
  return quota.ordered
    .filter(
      (provider) =>
        provider.kind === "primary" &&
        services[provider.provider].quota &&
        services[provider.provider].hud,
    )
    .map(buildRow);
});

const hudRef = ref<HTMLElement | null>(null);
let observer: ResizeObserver | null = null;
let reported = { width: 0, height: 0 };

function reportSize(): void {
  const element = hudRef.value;
  if (!element) {
    return;
  }
  const rect = element.getBoundingClientRect();
  const width = Math.ceil(rect.width);
  const height = Math.ceil(rect.height);
  if (width === reported.width && height === reported.height) {
    return;
  }
  reported = { width, height };
  void setFloatingSize(width, height);
}

/** 整个 HUD 都是拖动区：没有可点击控件，左键按下即开始拖动窗口。 */
function handleMouseDown(event: MouseEvent): void {
  if (event.button !== 0) {
    return;
  }
  void getCurrentWindow()
    .startDragging()
    .catch(() => {
      // 纯浏览器预览没有 Tauri 窗口桥。
    });
}

function handleContextMenu(): void {
  void showFloatingContextMenu();
}

onMounted(() => {
  document.body.dataset.surface = "floating";
  reportSize();
  if (hudRef.value) {
    observer = new ResizeObserver(reportSize);
    observer.observe(hudRef.value);
  }
});

onBeforeUnmount(() => {
  observer?.disconnect();
  observer = null;
});
</script>

<template>
  <div
    ref="hudRef"
    class="hud"
    role="group"
    :aria-label="t('floating.title')"
    @mousedown="handleMouseDown"
    @contextmenu.prevent="handleContextMenu"
  >
    <p v-if="rows.length === 0" class="hud__empty">{{ t("floating.empty") }}</p>

    <div
      v-for="row in rows"
      :key="row.key"
      class="hud__row"
      role="img"
      :aria-label="row.label"
      :title="row.label"
    >
      <span class="hud__mark" :class="`hud__mark--${row.provider}`" aria-hidden="true">{{
        row.mark
      }}</span>

      <!-- 无上限额度没有可填充的比例：留出轨道位保持三段对齐，右侧只写 ∞ -->
      <span class="hud__track" :class="{ 'hud__track--blank': row.unlimited }" aria-hidden="true">
        <span
          v-if="row.fill !== null"
          class="hud__fill"
          :class="`hud__fill--${row.tone}`"
          :style="{ inlineSize: `${row.fill}%` }"
        />
      </span>

      <span class="hud__value numeric" :class="`hud__value--${row.tone}`" aria-hidden="true">{{
        row.text
      }}</span>
    </div>
  </div>
</template>

<style>
/* 悬浮窗是透明窗口，形状由 .hud 自己绘制；全局基线的最小宽度与实色背景在这里都不适用 */
html body[data-surface="floating"] {
  background: transparent;
  overflow: hidden;
  user-select: none;
  -webkit-user-select: none;
}

html body[data-surface="floating"],
html body[data-surface="floating"] #app {
  min-width: 0;
}
</style>

<style scoped>
.hud {
  display: grid;
  gap: 7px;
  inline-size: max-content;
  min-inline-size: 168px;
  padding: 10px 14px;
  background: var(--surface-raised);
  border: 1px solid var(--border-hairline);
  border-radius: 14px;
  cursor: default;
}

.hud__empty {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.hud__row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.hud__mark {
  display: inline-flex;
  flex: none;
  align-items: center;
  justify-content: center;
  inline-size: 18px;
  block-size: 18px;
  border-radius: 5px;
  color: #ffffff;
  font-size: 0.5625rem;
  font-weight: 650;
  letter-spacing: 0;
}

.hud__mark--codex {
  background: var(--cat-codex);
}

.hud__mark--claude {
  background: var(--cat-claude);
}

.hud__mark--antigravity {
  background: var(--cat-antigravity);
}

.hud__mark--cursor {
  background: var(--cat-cursor);
}

.hud__mark--commandCode {
  background: var(--cat-commandcode);
}

.hud__track {
  flex: 1 1 auto;
  min-inline-size: 56px;
  block-size: 4px;
  background: var(--track-background);
  border-radius: 2px;
  overflow: hidden;
}

.hud__track--blank {
  background: transparent;
}

.hud__fill {
  display: block;
  block-size: 100%;
  background: var(--text-primary);
  border-radius: inherit;
}

.hud__fill--warning {
  background: var(--status-warning);
}

.hud__fill--low {
  background: var(--status-low);
}

.hud__fill--danger {
  background: var(--status-error);
}

.hud__fill--none {
  background: color-mix(in srgb, var(--text-primary) 38%, transparent);
}

/* 读数定宽：按「100%」定宽，各行进度条才等长；∞ 同宽 */
.hud__value {
  flex: none;
  inline-size: 38px;
  color: var(--text-primary);
  font-size: 0.8125rem;
  font-weight: 600;
  letter-spacing: -0.02em;
  text-align: end;
  white-space: nowrap;
}

.hud__value--none {
  color: var(--text-secondary);
}

.hud__value--warning {
  color: var(--status-warning);
}

.hud__value--low {
  color: var(--status-low);
}

.hud__value--danger {
  color: var(--status-error);
}

@media (prefers-reduced-motion: no-preference) {
  .hud__fill {
    transition: inline-size var(--motion-base) var(--ease-out);
  }
}
</style>
