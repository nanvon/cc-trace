<script setup lang="ts">
/**
 * 主窗口 Conversations 分栏视图（对齐 cc-bar F-17 分栏形态）。
 *
 * 左侧：标题／项目搜索、项目筛选、Provider 联动、时间范围（与用量页共享）、
 * Recent／Tokens／Cost 排序与分页；右侧：选中对话的全生命周期详情面板。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";

import ConversationDetailPane from "../components/ConversationDetailPane.vue";
import MenuSelect, { type MenuSelectOption } from "../components/MenuSelect.vue";
import { useSettingsStore } from "../features/settings/store";
import type {
  UsageConversation,
  UsageConversationPage,
  UsageConversationProjectOption,
  UsageConversationSort,
} from "../features/usage/contracts";
import { USAGE_SOURCES } from "../features/usage/contracts";
import { listConversationProjects, listConversations } from "../features/usage/api";
import { formatUsageCost, presentUsageTokens } from "../features/usage/presentation";
import { useUsageStore } from "../features/usage/store";

const PAGE_SIZE = 20;

const { t, locale } = useI18n();
const settings = useSettingsStore();
const usage = useUsageStore();

const loading = ref(true);
const unavailable = ref(false);
const page = ref<UsageConversationPage | null>(null);
const search = ref("");
const sort = ref<UsageConversationSort>("recent");
const offset = ref(0);
const pendingSearch = ref("");
const selectedKey = ref<string | null>(null);
const projects = ref<UsageConversationProjectOption[]>([]);
const projectFilter = ref<string | null>(null);

const SORT_OPTIONS: Array<{ value: UsageConversationSort; label: string }> = [
  { value: "recent", label: "conversations.sort.recent" },
  { value: "tokens", label: "conversations.sort.tokens" },
  { value: "cost", label: "conversations.sort.cost" },
];

const sortOptions = computed<MenuSelectOption<UsageConversationSort>[]>(() =>
  SORT_OPTIONS.map((option) => ({ value: option.value, label: t(option.label) })),
);

/** 项目菜单：首项「全部项目」，其余带对话计数（对齐 cc-bar menuLabel）。 */
const projectOptions = computed<MenuSelectOption<string>[]>(() => [
  { value: "", label: t("conversations.projectFilter") },
  ...projects.value.map((option) => ({
    value: option.name,
    label: option.name,
    count: option.conversationCount,
  })),
]);

const visibleSources = computed(() => {
  const visibility = settings.settings?.usageServiceVisibility;
  if (!visibility) return [...USAGE_SOURCES];
  return USAGE_SOURCES.filter((source) => visibility[source]);
});

const allServicesOff = computed(() => visibleSources.value.length === 0);
const total = computed(() => page.value?.total ?? 0);
const hasNext = computed(() => (offset.value + 1) * PAGE_SIZE < total.value);
const hasPrevious = computed(() => offset.value > 0);
let loadRequest = 0;

/** 与用量页共享的全局时间范围（ADR-0024 数据源过滤同理）。 */
function conversationFilter() {
  const range = usage.dashboardRange;
  return {
    from: range.preset === "all" ? null : range.from,
    to: range.preset === "all" ? null : range.to,
    source: usage.sourceFilter === "all" ? null : usage.sourceFilter,
    model: null,
    speed: null,
  };
}
function queryArgs() {
  const sources =
    usage.sourceFilter === "all" && visibleSources.value.length > 0 ? visibleSources.value : null;
  return {
    filter: conversationFilter(),
    search: pendingSearch.value || null,
    project: projectFilter.value === "" ? null : projectFilter.value,
    sort: sort.value,
    sources,
    limit: null,
    offset: null,
  };
}
async function loadProjects(): Promise<void> {
  try {
    const options = await listConversationProjects(queryArgs());
    projects.value = options;
    if (projectFilter.value && !options.some((option) => option.name === projectFilter.value)) {
      projectFilter.value = null;
    }
  } catch {
    // 项目菜单失败只降级为「全部项目」，不阻断列表。
    projects.value = [];
    projectFilter.value = null;
  }
}

async function load(): Promise<void> {
  const request = ++loadRequest;
  loading.value = true;
  unavailable.value = false;
  try {
    const result = await listConversations({
      ...queryArgs(),
      limit: PAGE_SIZE,
      offset: offset.value,
    });
    if (request === loadRequest) {
      page.value = result;
      if (
        selectedKey.value &&
        !result.items.some((item) => item.conversationKey === selectedKey.value)
      ) {
        selectedKey.value = null;
      }
    }
  } catch {
    if (request === loadRequest) {
      unavailable.value = true;
    }
  } finally {
    if (request === loadRequest) {
      loading.value = false;
    }
  }
}

/** 只重载列表：搜索、排序与分页不影响项目集合（conversation_projects 不使用这些条件）。 */
function reload(): void {
  offset.value = 0;
  void load();
}

/** 列表与项目集合一起重载：时间范围、数据源或当前项目选择变化时使用。 */
function reloadWithProjects(): void {
  offset.value = 0;
  void load();
  void loadProjects();
}

let searchTimer: ReturnType<typeof setTimeout> | null = null;

/** 即时搜索（对齐 cc-bar）：输入防抖后即过滤，无提交按钮。 */
watch(search, (value) => {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    pendingSearch.value = value.trim();
    reload();
  }, 300);
});

// projectFilter 变化必须重载项目集合：loadProjects 内含「选中项目已不在新选项中则清空筛选」。
watch(projectFilter, () => {
  reloadWithProjects();
});

watch(sort, () => {
  reload();
});

function goPrevious(): void {
  if (!hasPrevious.value) return;
  offset.value -= PAGE_SIZE;
  void load();
}

function goNext(): void {
  if (!hasNext.value) return;
  offset.value += PAGE_SIZE;
  void load();
}

function selectConversation(conversation: UsageConversation): void {
  selectedKey.value = conversation.conversationKey;
}

function formatTime(value: string): string {
  return new Intl.DateTimeFormat(locale.value, {
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    month: "numeric",
  }).format(new Date(value));
}

function tokensOf(conversation: UsageConversation): string {
  const display = presentUsageTokens(locale.value, conversation.tokens.totalTokens);
  return `${display.value}${display.unit}`;
}

function costOf(conversation: UsageConversation): string {
  return (
    formatUsageCost(
      locale.value,
      conversation.cost,
      conversation.entryCount,
      t("main.lessThanCent"),
    ) ?? t("main.unpriced")
  );
}

/** 速度徽标：Fast 专用、混合、其余不显示（对齐 cc-bar UsageSpeedBadge）。 */
function speedBadge(conversation: UsageConversation): "fast" | "mixed" | null {
  const fast = conversation.fast.rawTokens;
  if (fast <= 0) return null;
  return fast < conversation.tokens.totalTokens ? "mixed" : "fast";
}

function pageLabel(): string {
  if (total.value === 0) return "";
  const first = offset.value + 1;
  const last = Math.min(offset.value + PAGE_SIZE, total.value);
  return t("conversations.pageRange", { first, last, total: total.value });
}

onMounted(() => {
  if (visibleSources.value.length > 0) {
    void load();
    void loadProjects();
  }
});

onBeforeUnmount(() => {
  if (searchTimer) {
    clearTimeout(searchTimer);
    searchTimer = null;
  }
});

// 侧边栏数据源组是全局状态（ADR-0024）：变化时重新加载对话列表与项目集合。
watch(
  () => usage.sourceFilter,
  () => {
    offset.value = 0;
    void load();
    void loadProjects();
  },
);

// 与用量页共享时间范围：范围变化时列表与项目菜单跟随刷新。
watch(
  () => usage.dashboardRange,
  () => {
    offset.value = 0;
    void load();
    void loadProjects();
  },
);
</script>

<template>
  <main class="conversations" :aria-label="t('a11y.conversationsRegion')">
    <div class="conversations__inner">
      <header class="conversations__header">
        <h1 id="conversations-title" tabindex="-1">{{ t("conversations.title") }}</h1>
      </header>

      <div class="conversations__split">
        <section class="conversations__list-col" :aria-label="t('conversations.filter')">
          <div class="conversations__filters" role="group">
            <div class="conversations__search">
              <svg class="conversations__search-icon" viewBox="0 0 16 16" aria-hidden="true">
                <circle
                  cx="7"
                  cy="7"
                  r="4.5"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.5"
                ></circle>
                <path
                  d="M10.5 10.5 14 14"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linecap="round"
                ></path>
              </svg>
              <input
                v-model="search"
                type="search"
                :placeholder="t('conversations.searchPlaceholder')"
                :aria-label="t('conversations.searchPlaceholder')"
                autocomplete="off"
              />
            </div>

            <MenuSelect
              v-model="projectFilter"
              :options="projectOptions"
              :label="t('conversations.filterByProject')"
              :empty-label="t('conversations.projectFilter')"
            />

            <MenuSelect
              v-model="sort"
              :options="sortOptions"
              :label="t('conversations.sortLabel')"
            />
          </div>

          <p v-if="allServicesOff" class="conversations__notice">
            {{ t("conversations.allServicesOff") }}
          </p>
          <p v-else-if="unavailable" class="conversations__notice">
            {{ t("conversations.unavailable") }}
          </p>
          <p v-else-if="loading" class="conversations__notice">{{ t("conversations.loading") }}</p>
          <p v-else-if="total === 0" class="conversations__notice">
            {{ t("conversations.empty") }}
          </p>

          <template v-else-if="page">
            <ul class="conversations__list">
              <li v-for="conversation in page.items" :key="conversation.conversationKey">
                <button
                  type="button"
                  class="conversations__row"
                  :class="{ on: selectedKey === conversation.conversationKey }"
                  :data-provider="conversation.source"
                  :aria-current="selectedKey === conversation.conversationKey ? 'true' : undefined"
                  @click="selectConversation(conversation)"
                >
                  <span class="conversations__dot" aria-hidden="true"></span>
                  <span class="conversations__main">
                    <strong class="conversations__title">
                      {{ conversation.title ?? t("conversations.untitled") }}
                    </strong>
                    <small class="conversations__meta">
                      <span v-if="conversation.projectHint">{{ conversation.projectHint }}</span>
                      <span v-else-if="conversation.isSidechain">{{
                        t("conversations.sidechain")
                      }}</span>
                      <span v-if="conversation.models.length > 0">{{
                        conversation.models.join(", ")
                      }}</span>
                      <span v-if="speedBadge(conversation)" class="conversations__speed">
                        {{
                          speedBadge(conversation) === "fast"
                            ? t("conversations.speedFast")
                            : t("conversations.speedMixed")
                        }}
                      </span>
                      <span>{{ formatTime(conversation.lastAt) }}</span>
                    </small>
                  </span>
                  <span class="conversations__stats">
                    <span class="numeric">{{ conversation.entryCount }}</span>
                    <span class="numeric">{{ tokensOf(conversation) }}</span>
                    <strong class="numeric">{{ costOf(conversation) }}</strong>
                  </span>
                </button>
              </li>
            </ul>

            <footer class="conversations__footer">
              <span class="conversations__page">{{ pageLabel() }}</span>
              <div class="conversations__pager">
                <button
                  type="button"
                  class="button button--quiet"
                  :disabled="!hasPrevious"
                  @click="goPrevious"
                >
                  {{ t("conversations.previous") }}
                </button>
                <button
                  type="button"
                  class="button button--quiet"
                  :disabled="!hasNext"
                  @click="goNext"
                >
                  {{ t("conversations.next") }}
                </button>
              </div>
            </footer>
          </template>
        </section>

        <aside class="conversations__detail-col" :aria-label="t('a11y.conversationDetailRegion')">
          <ConversationDetailPane :conversation-key="selectedKey" />
        </aside>
      </div>
    </div>
  </main>
</template>

<style scoped>
.conversations {
  --usage-canvas: var(--surface-primary);
  --usage-surface: var(--surface-raised);
  --usage-divider: var(--border-subtle);
  container-type: inline-size;
  min-block-size: 100vh;
  padding: clamp(1.125rem, 3vw, 1.375rem) clamp(1.125rem, 3vw, 1.875rem) 2.125rem;
  background: var(--usage-canvas);
  font-family: var(--font-ui);
}

.conversations__inner {
  inline-size: min(100%, 75rem);
  margin-inline: auto;
}

.conversations__header {
  display: flex;
  align-items: baseline;
  gap: var(--space-4);
  padding-block-end: 0.75rem;
  border-block-end: 1px solid var(--usage-divider);
  margin-block-end: 1.25rem;
}

.conversations__header h1 {
  margin: 0;
  font-size: 1.5rem;
  font-weight: 700;
  letter-spacing: -0.025em;
  line-height: 1.15;
}

.conversations__header h1[tabindex="-1"]:focus {
  outline: none;
}

.conversations__split {
  display: grid;
  grid-template-columns: minmax(24rem, 5fr) minmax(20rem, 4fr);
  gap: 1.25rem;
  align-items: start;
}

.conversations__list-col {
  min-inline-size: 0;
}

.conversations__detail-col {
  min-inline-size: 0;
  position: sticky;
  top: 0;
  max-block-size: calc(100vh - 6.5rem);
  overflow-y: auto;
  overscroll-behavior: contain;
  border-inline-start: 1px solid var(--usage-divider);
}

.conversations__filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  margin-block-end: 0.875rem;
}

/* 搜索框：放大镜图标内嵌，控件与 MenuSelect 同高、同边框家族（对齐用量页 date-input） */
.conversations__search {
  position: relative;
}

.conversations__search-icon {
  position: absolute;
  inset-inline-start: 0.625rem;
  inset-block-start: 50%;
  inline-size: 0.875rem;
  block-size: 0.875rem;
  color: var(--text-secondary);
  transform: translateY(-50%);
  pointer-events: none;
}

.conversations__search input {
  inline-size: 14rem;
  min-block-size: 2.25rem;
  padding: 0 0.625rem 0 2rem;
  border: 1px solid var(--border-subtle);
  border-radius: 0.5625rem;
  color: var(--text-primary);
  background: var(--usage-surface);
  font-size: 0.75rem;
}

.conversations__search input::-webkit-search-cancel-button {
  display: none;
}

.conversations__notice {
  padding: 2.5rem 1rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

.conversations__list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.conversations__row {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  inline-size: 100%;
  min-block-size: 3.5rem;
  padding: 0.5rem 0.875rem;
  border: 1px solid var(--border-hairline);
  border-block-start-width: 0;
  color: inherit;
  background: var(--usage-surface);
  font-size: 0.75rem;
  text-align: start;
}

.conversations__list li:first-child .conversations__row {
  border-block-start-width: 1px;
  border-start-start-radius: 0.75rem;
  border-start-end-radius: 0.75rem;
}

.conversations__list li:last-child .conversations__row {
  border-end-start-radius: 0.75rem;
  border-end-end-radius: 0.75rem;
}

/* hover 用中性浅底加深（去掉品牌色混合，与用量页 hover 同语法） */
.conversations__row:hover {
  background: color-mix(in srgb, var(--text-primary) 5%, transparent);
}

/* 选中：中性浅底 + 左侧 3px 主色竖条 + 描边保持，与侧边栏「蓝底白字」形成两级选中 */
.conversations__row.on {
  background: var(--action-soft);
  box-shadow: inset 3px 0 0 0 var(--action-primary);
}

.conversations__dot {
  inline-size: 0.4375rem;
  block-size: 0.4375rem;
  flex: 0 0 auto;
  border-radius: 0.125rem;
  background: var(--cat-codex);
}

.conversations__row[data-provider="claude"] .conversations__dot {
  background: var(--cat-claude);
}

.conversations__row[data-provider="pi"] .conversations__dot {
  background: var(--cat-pi);
}

.conversations__row[data-provider="opencode"] .conversations__dot {
  background: var(--cat-opencode);
}

.conversations__main {
  display: flex;
  flex-direction: column;
  gap: 0.1875rem;
  min-inline-size: 0;
  flex: 1 1 auto;
}

.conversations__title {
  overflow: hidden;
  font-size: 0.8125rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conversations__meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.625rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

/* 速度徽标：中性胶囊（对齐 cc-bar UsageSpeedBadge），不占交互色 */
.conversations__speed {
  padding: 0.0625rem 0.375rem;
  border-radius: 999px;
  background: color-mix(in srgb, var(--text-secondary) 12%, transparent);
  color: var(--text-secondary);
  font-size: 0.65625rem;
  font-weight: 600;
}

.conversations__stats {
  display: flex;
  align-items: baseline;
  gap: 1rem;
  flex: 0 0 auto;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-variant-numeric: tabular-nums;
}

.conversations__stats strong {
  color: var(--text-primary);
}

.conversations__footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  margin-block-start: 0.75rem;
}

.conversations__page {
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.conversations__pager {
  display: flex;
  gap: 0.375rem;
}

@container (max-width: 860px) {
  .conversations__split {
    grid-template-columns: 1fr;
  }

  .conversations__detail-col {
    position: static;
    max-block-size: none;
    border-inline-start: 0;
    border-block-start: 1px solid var(--usage-divider);
  }
}

@container (max-width: 640px) {
  .conversations__stats {
    display: none;
  }

  .conversations__search input {
    inline-size: 10rem;
  }
}
</style>
