<script setup lang="ts">
/**
 * 主窗口 Projects 分栏视图（对齐 cc-bar 项目分析页，ADR-0034 项目身份）。
 *
 * 左侧：搜索、粒度与范围（与用量页／对话页共享同一份状态）、排序、项目列表
 * （特殊行「无明确项目」「系统任务」在普通项目之后，未归属分组固定在最后）；
 * 右侧：选中项目的详情。Cursor 没有项目信息，侧边栏选中 Cursor 时回退到「全部」。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";

import MenuSelect, { type MenuSelectOption } from "../components/MenuSelect.vue";
import ProjectDetailPane from "../components/ProjectDetailPane.vue";
import { listProjects } from "../features/usage/api";
import type {
  UsageGranularity,
  UsageProjectSort,
  UsageProjectSummary,
} from "../features/usage/contracts";
import { formatUsageCost, presentUsageTokens } from "../features/usage/presentation";
import { usageDashboardRanges, usageGranularityPresets } from "../features/usage/ranges";
import type { UsageRangePreset } from "../features/usage/ranges";
import { useUsageStore } from "../features/usage/store";
import { usePrivacy } from "../lib/privacy";

const LIST_LIMIT = 200;
const KEY_NONE = "@none";
const KEY_SYSTEM = "@system";

const { t, locale } = useI18n();
const usage = useUsageStore();
const privacy = usePrivacy();

const loading = ref(true);
const unavailable = ref(false);
const items = ref<UsageProjectSummary[]>([]);
const search = ref("");
const pendingSearch = ref("");
const sort = ref<UsageProjectSort>("cost");
const route = useRoute();

/** 概览构成面板跳来：`?project=<key>` 选中项目，`?unattributed=1` 选中未归属（key 空串）。 */
const routedKey =
  route.query.unattributed === "1"
    ? ""
    : typeof route.query.project === "string" && route.query.project !== ""
      ? route.query.project
      : null;
const selectedKey = ref<string | null>(routedKey);
const refreshToken = ref(0);
let loadRequest = 0;

const GRANULARITIES: UsageGranularity[] = ["day", "week", "month"];

const sortOptions = computed<MenuSelectOption<UsageProjectSort>[]>(() => [
  { value: "cost", label: t("projects.sort.cost") },
  { value: "tokens", label: t("projects.sort.tokens") },
  { value: "recent", label: t("projects.sort.recent") },
]);

const presets = computed<UsageRangePreset[]>(() => usageGranularityPresets(usage.granularity));
const allServicesOff = computed(() => usage.dashboardSources.length === 0);

/** 普通项目按后端排序；特殊行随后；未归属固定最后。 */
const ordered = computed(() => {
  const rank = (item: UsageProjectSummary): number =>
    item.unattributed ? 3 : item.key === KEY_SYSTEM ? 2 : item.key === KEY_NONE ? 1 : 0;
  return items.value
    .map((item, index) => ({ item, index }))
    .sort((left, right) => rank(left.item) - rank(right.item) || left.index - right.index)
    .map((entry) => entry.item);
});

const selected = computed(() => items.value.find((item) => item.key === selectedKey.value) ?? null);

const firstScan = computed(() => usage.scanning && !usage.status?.finishedAt);
const scanText = computed(() => {
  if (usage.scanning) return t("main.scanningUsage");
  const finishedAt = usage.status?.finishedAt;
  if (!finishedAt) return t("main.neverScanned");
  return t("main.lastScan", {
    time: new Intl.DateTimeFormat(locale.value, {
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      month: "numeric",
    }).format(new Date(finishedAt)),
  });
});
const firstScanText = computed(() =>
  t("projects.firstScan", {
    done: usage.status?.completedFiles ?? 0,
    total: usage.status?.discoveredFiles ?? 0,
  }),
);

async function load(): Promise<void> {
  const request = ++loadRequest;
  const sources = usage.dashboardSources;
  if (sources.length === 0) {
    items.value = [];
    loading.value = false;
    return;
  }
  loading.value = true;
  unavailable.value = false;
  const range = usage.dashboardRange;
  try {
    const page = await listProjects({
      filter: {
        from: range.preset === "all" ? null : range.from,
        to: range.preset === "all" ? null : range.to,
        sources: [...sources],
        model: null,
        speed: null,
        project: null,
      },
      search: pendingSearch.value || null,
      sort: sort.value,
      limit: LIST_LIMIT,
      offset: 0,
    });
    if (request !== loadRequest) return;
    items.value = page.items;
    if (selectedKey.value !== null && !page.items.some((item) => item.key === selectedKey.value)) {
      selectedKey.value = null;
    }
  } catch {
    if (request === loadRequest) unavailable.value = true;
  } finally {
    if (request === loadRequest) loading.value = false;
  }
}

let searchTimer: ReturnType<typeof setTimeout> | null = null;
watch(search, (value) => {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    pendingSearch.value = value.trim();
    void load();
  }, 300);
});
watch(sort, () => void load());
watch(
  () => [usage.dashboardRange, usage.sourceFilter, usage.visibleSources.join(",")],
  () => void load(),
);

// Cursor 没有项目信息：选中时回到「全部」。
watch(
  () => usage.sourceFilter,
  (source) => {
    if (source === "cursor") usage.selectSource("all");
  },
);

// 扫描完成（finishedAt 变化）后重载列表与详情。
watch(
  () => usage.status?.finishedAt,
  (next, previous) => {
    if (next && next !== previous) {
      refreshToken.value += 1;
      void load();
    }
  },
);

let poll: ReturnType<typeof setInterval> | null = null;

async function refreshUsage(): Promise<void> {
  if (usage.scanning) return;
  await usage.startScan();
}

onMounted(() => {
  if (usage.sourceFilter === "cursor") usage.selectSource("all");
  void load();
  poll = setInterval(() => {
    if (usage.scanning) void usage.poll();
  }, 1500);
});

onBeforeUnmount(() => {
  if (searchTimer) clearTimeout(searchTimer);
  if (poll) clearInterval(poll);
});

function selectGranularity(granularity: UsageGranularity): void {
  void usage.setGranularity(granularity);
}

function selectPreset(preset: UsageRangePreset): void {
  void usage.loadDashboard(usageDashboardRanges()[preset]);
}

function rowName(item: UsageProjectSummary): string {
  if (item.unattributed) return t("projects.special.unattributed");
  if (item.key === KEY_NONE) return t("projects.special.none");
  if (item.key === KEY_SYSTEM) return t("projects.special.system");
  return item.name;
}

function rowSubtitle(item: UsageProjectSummary): string {
  if (item.unattributed) return t("projects.special.unattributedHint");
  if (item.key === KEY_NONE) return t("projects.special.noneHint");
  if (item.key === KEY_SYSTEM) return t("projects.special.systemHint");
  return privacy.path(item.path);
}

function badges(item: UsageProjectSummary): string[] {
  const values: string[] = [];
  if (item.worktreeCount > 0) {
    values.push(t("projects.badge.worktrees", { count: item.worktreeCount }));
  }
  if (item.status === "unavailable") values.push(t("projects.badge.missing"));
  if (item.status === "available" && item.isGit === false) values.push(t("projects.badge.notGit"));
  return values;
}

function tokensOf(item: UsageProjectSummary): string {
  const display = presentUsageTokens(locale.value, item.tokens.totalTokens);
  return `${display.value}${display.unit}`;
}

function costOf(item: UsageProjectSummary): string {
  return (
    formatUsageCost(locale.value, item.cost, item.entryCount, t("main.lessThanCent")) ??
    t("main.unpriced")
  );
}
</script>

<template>
  <main class="projects" :aria-label="t('a11y.projectsRegion')">
    <div class="projects__inner">
      <header class="projects__header">
        <h1 id="main-projects-title" tabindex="-1">{{ t("projects.title") }}</h1>
        <div class="projects__tools">
          <div
            class="projects__segmented"
            role="group"
            :aria-label="t('projects.granularity.label')"
          >
            <button
              v-for="granularity in GRANULARITIES"
              :key="granularity"
              type="button"
              :aria-pressed="usage.granularity === granularity"
              :data-selected="usage.granularity === granularity ? 'true' : undefined"
              @click="selectGranularity(granularity)"
            >
              {{ t(`projects.granularity.${granularity}`) }}
            </button>
          </div>
          <div class="projects__segmented" role="group" :aria-label="t('projects.rangeLabel')">
            <button
              v-for="preset in presets"
              :key="preset"
              type="button"
              :aria-pressed="usage.dashboardRange.preset === preset"
              :data-selected="usage.dashboardRange.preset === preset ? 'true' : undefined"
              @click="selectPreset(preset)"
            >
              {{ t(`main.range.${preset}`) }}
            </button>
          </div>
          <span class="projects__scan">{{ scanText }}</span>
          <button
            type="button"
            class="button button--quiet"
            :disabled="usage.scanning"
            @click="refreshUsage"
          >
            {{ usage.scanning ? t("main.scanningUsage") : t("main.refreshUsage") }}
          </button>
        </div>
      </header>

      <p v-if="firstScan" class="projects__banner" role="status">{{ firstScanText }}</p>
      <p v-else-if="usage.scanning" class="projects__banner projects__banner--quiet" role="status">
        {{ t("projects.scanning") }}
      </p>

      <div class="projects__split">
        <section class="projects__list-col" :aria-label="t('projects.filter')">
          <div class="projects__filters" role="group">
            <div class="projects__search">
              <input
                v-model="search"
                type="search"
                :placeholder="t('projects.searchPlaceholder')"
                :aria-label="t('projects.searchPlaceholder')"
                autocomplete="off"
              />
            </div>
            <MenuSelect v-model="sort" :options="sortOptions" :label="t('projects.sortLabel')" />
          </div>

          <p v-if="allServicesOff" class="projects__notice">{{ t("projects.allServicesOff") }}</p>
          <p v-else-if="unavailable" class="projects__notice">{{ t("projects.unavailable") }}</p>
          <p v-else-if="loading && items.length === 0" class="projects__notice">
            {{ t("projects.loading") }}
          </p>
          <p v-else-if="items.length === 0" class="projects__notice">{{ t("projects.empty") }}</p>

          <ul v-else class="projects__list">
            <li v-for="item in ordered" :key="item.key">
              <button
                type="button"
                class="projects__row"
                :class="{
                  on: selectedKey === item.key,
                  'projects__row--special': item.unattributed || item.key.startsWith('@'),
                }"
                :aria-current="selectedKey === item.key ? 'true' : undefined"
                @click="selectedKey = item.key"
              >
                <span class="projects__main">
                  <strong class="projects__name">{{ rowName(item) }}</strong>
                  <small class="projects__meta">
                    <span class="projects__path">{{ rowSubtitle(item) }}</span>
                    <span v-for="badge in badges(item)" :key="badge" class="projects__badge">{{
                      badge
                    }}</span>
                  </small>
                </span>
                <span class="projects__stats">
                  <span class="numeric">{{
                    t("projects.conversationsCount", { count: item.conversationCount })
                  }}</span>
                  <span class="numeric">{{ tokensOf(item) }}</span>
                  <strong class="numeric">{{ costOf(item) }}</strong>
                </span>
              </button>
            </li>
          </ul>
        </section>

        <aside class="projects__detail-col" :aria-label="t('a11y.projectDetailRegion')">
          <ProjectDetailPane :project="selected" :refresh-token="refreshToken" />
        </aside>
      </div>
    </div>
  </main>
</template>

<style scoped>
.projects {
  container-type: inline-size;
  min-block-size: 100vh;
  padding: clamp(1.125rem, 3vw, 1.375rem) clamp(1.125rem, 3vw, 1.875rem) 2.125rem;
  background: var(--surface-primary);
  font-family: var(--font-ui);
}

.projects__inner {
  inline-size: min(100%, 75rem);
  margin-inline: auto;
}

.projects__header {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding-block-end: 0.75rem;
  border-block-end: 1px solid var(--border-subtle);
  margin-block-end: 1.25rem;
}

.projects__header h1 {
  margin: 0;
  font-size: 1.5rem;
  font-weight: 700;
  letter-spacing: -0.025em;
  line-height: 1.15;
}

.projects__header h1[tabindex="-1"]:focus {
  outline: none;
}

.projects__tools {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.625rem;
}

.projects__segmented {
  display: inline-flex;
  padding: 0.125rem;
  border-radius: 0.5625rem;
  background: var(--track-background);
}

.projects__segmented button {
  min-block-size: 1.75rem;
  padding: 0 0.625rem;
  border: 0;
  border-radius: 0.4375rem;
  color: var(--text-secondary);
  background: transparent;
  font-size: 0.6875rem;
}

.projects__segmented button[data-selected="true"] {
  color: var(--text-primary);
  background: var(--surface-raised);
  box-shadow: 0 1px 2px rgb(24 24 27 / 10%);
  font-weight: 570;
}

.projects__scan {
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.projects__banner {
  margin: 0 0 0.875rem;
  padding: 0.5rem 0.75rem;
  border-radius: 0.5625rem;
  background: var(--action-soft);
  font-size: 0.75rem;
}

.projects__banner--quiet {
  color: var(--text-secondary);
  background: transparent;
  padding-inline: 0;
  font-size: 0.6875rem;
}

.projects__split {
  display: grid;
  grid-template-columns: minmax(24rem, 5fr) minmax(20rem, 5fr);
  gap: 1.25rem;
  align-items: start;
}

.projects__list-col {
  min-inline-size: 0;
}

.projects__detail-col {
  min-inline-size: 0;
  position: sticky;
  top: 0;
  max-block-size: calc(100vh - 6.5rem);
  overflow-y: auto;
  overscroll-behavior: contain;
  border-inline-start: 1px solid var(--border-subtle);
}

.projects__filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  margin-block-end: 0.875rem;
}

.projects__search input {
  inline-size: 14rem;
  min-block-size: 2.25rem;
  padding: 0 0.625rem;
  border: 1px solid var(--border-subtle);
  border-radius: 0.5625rem;
  color: var(--text-primary);
  background: var(--surface-raised);
  font-size: 0.75rem;
}

.projects__notice {
  padding: 2.5rem 1rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

.projects__list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.projects__row {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  inline-size: 100%;
  min-block-size: 3.5rem;
  padding: 0.5rem 0.875rem;
  border: 1px solid var(--border-hairline);
  border-block-start-width: 0;
  color: inherit;
  background: var(--surface-raised);
  font-size: 0.75rem;
  text-align: start;
}

.projects__list li:first-child .projects__row {
  border-block-start-width: 1px;
  border-start-start-radius: 0.75rem;
  border-start-end-radius: 0.75rem;
}

.projects__list li:last-child .projects__row {
  border-end-start-radius: 0.75rem;
  border-end-end-radius: 0.75rem;
}

.projects__row:hover {
  background: color-mix(in srgb, var(--text-primary) 5%, transparent);
}

.projects__row.on {
  background: var(--action-soft);
  box-shadow: inset 3px 0 0 0 var(--action-primary);
}

.projects__row--special .projects__name {
  color: var(--text-secondary);
}

.projects__main {
  display: flex;
  flex-direction: column;
  gap: 0.1875rem;
  min-inline-size: 0;
  flex: 1 1 auto;
}

.projects__name {
  overflow: hidden;
  font-size: 0.8125rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.projects__meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.projects__path {
  min-inline-size: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.projects__badge {
  padding: 0.0625rem 0.375rem;
  border-radius: 999px;
  background: color-mix(in srgb, var(--text-secondary) 12%, transparent);
  font-size: 0.65625rem;
  font-weight: 600;
  white-space: nowrap;
}

.projects__stats {
  display: flex;
  align-items: baseline;
  gap: 1rem;
  flex: 0 0 auto;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-variant-numeric: tabular-nums;
}

.projects__stats strong {
  color: var(--text-primary);
}

@container (max-width: 860px) {
  .projects__split {
    grid-template-columns: 1fr;
  }

  .projects__detail-col {
    position: static;
    max-block-size: none;
    border-inline-start: 0;
    border-block-start: 1px solid var(--border-subtle);
  }
}

@container (max-width: 640px) {
  .projects__stats span:first-child {
    display: none;
  }

  .projects__search input {
    inline-size: 10rem;
  }
}
</style>
