<script setup lang="ts">
/**
 * 主窗口 Conversations 列表视图。
 *
 * 标题／项目搜索、Provider 筛选、Recent／Tokens／Cost 排序与分页；对应 cc-bar F-17 的列表侧。
 * 行内展示 Provider、标题、项目、时间、请求数、Token 与费用；点击进入全生命周期详情。
 */
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";

import { navigateMain } from "../features/app/navigation";
import type {
  UsageConversation,
  UsageConversationPage,
  UsageConversationSort,
} from "../features/usage/contracts";
import { listConversations } from "../features/usage/api";
import { formatUsageCost, presentUsageTokens } from "../features/usage/presentation";

const PAGE_SIZE = 20;

const { t, locale } = useI18n();
const router = useRouter();

const loading = ref(true);
const unavailable = ref(false);
const page = ref<UsageConversationPage | null>(null);
const search = ref("");
const source = ref<"all" | "codex" | "claude">("all");
const sort = ref<UsageConversationSort>("recent");
const offset = ref(0);
const pendingSearch = ref("");

const SORT_OPTIONS: Array<{ value: UsageConversationSort; label: string }> = [
  { value: "recent", label: "conversations.sort.recent" },
  { value: "tokens", label: "conversations.sort.tokens" },
  { value: "cost", label: "conversations.sort.cost" },
];

const SOURCE_OPTIONS: Array<{ value: "all" | "codex" | "claude"; label: string }> = [
  { value: "all", label: "conversations.sourceAll" },
  { value: "codex", label: "provider.codex" },
  { value: "claude", label: "provider.claude" },
];

const total = computed(() => page.value?.total ?? 0);
const hasNext = computed(() => (offset.value + 1) * PAGE_SIZE < total.value);
const hasPrevious = computed(() => offset.value > 0);

async function load(): Promise<void> {
  loading.value = true;
  unavailable.value = false;
  try {
    page.value = await listConversations({
      filter: {
        from: null,
        to: null,
        source: source.value === "all" ? null : source.value,
        model: null,
        speed: null,
      },
      search: pendingSearch.value || null,
      project: null,
      sort: sort.value,
      limit: PAGE_SIZE,
      offset: offset.value,
    });
  } catch {
    unavailable.value = true;
  } finally {
    loading.value = false;
  }
}

function submitSearch(): void {
  pendingSearch.value = search.value.trim();
  offset.value = 0;
  void load();
}

function changeSource(): void {
  offset.value = 0;
  void load();
}

function changeSort(): void {
  offset.value = 0;
  void load();
}

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

function openDetail(conversation: UsageConversation): void {
  void router.push({ name: "conversation-detail", params: { key: conversation.conversationKey } });
}

function backToUsage(): void {
  void navigateMain(router, "quota", "usage-title");
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

function pageLabel(): string {
  if (total.value === 0) return "";
  const first = offset.value + 1;
  const last = Math.min(offset.value + PAGE_SIZE, total.value);
  return t("conversations.pageRange", { first, last, total: total.value });
}

onMounted(() => {
  void load();
});
</script>

<template>
  <main class="conversations" :aria-label="t('a11y.conversationsRegion')">
    <div class="conversations__inner">
      <header class="conversations__header">
        <button type="button" class="button button--quiet conversations__back" @click="backToUsage">
          <span aria-hidden="true">←</span>
          {{ t("conversations.backToUsage") }}
        </button>
        <h1 id="conversations-title" tabindex="-1">{{ t("conversations.title") }}</h1>
      </header>

      <div class="conversations__filters" role="group" :aria-label="t('conversations.filter')">
        <form class="conversations__search" @submit.prevent="submitSearch">
          <input
            v-model="search"
            type="search"
            :placeholder="t('conversations.searchPlaceholder')"
            :aria-label="t('conversations.searchPlaceholder')"
            autocomplete="off"
          />
          <button type="submit" class="button">{{ t("conversations.search") }}</button>
        </form>

        <label class="conversations__select">
          <span class="visually-hidden">{{ t("conversations.source") }}</span>
          <select :value="source" @change="changeSource">
            <option v-for="option in SOURCE_OPTIONS" :key="option.value" :value="option.value">
              {{ t(option.label) }}
            </option>
          </select>
        </label>

        <label class="conversations__select">
          <span class="visually-hidden">{{ t("conversations.sortLabel") }}</span>
          <select :value="sort" @change="changeSort">
            <option v-for="option in SORT_OPTIONS" :key="option.value" :value="option.value">
              {{ t(option.label) }}
            </option>
          </select>
        </label>
      </div>

      <p v-if="unavailable" class="conversations__notice">{{ t("conversations.unavailable") }}</p>
      <p v-else-if="loading" class="conversations__notice">{{ t("conversations.loading") }}</p>
      <p v-else-if="total === 0" class="conversations__notice">{{ t("conversations.empty") }}</p>

      <template v-else-if="page">
        <ul class="conversations__list">
          <li v-for="conversation in page.items" :key="conversation.conversationKey">
            <button
              type="button"
              class="conversations__row"
              :data-provider="conversation.source"
              @click="openDetail(conversation)"
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
            <button type="button" class="button button--quiet" :disabled="!hasNext" @click="goNext">
              {{ t("conversations.next") }}
            </button>
          </div>
        </footer>
      </template>
    </div>
  </main>
</template>

<style scoped>
.conversations {
  --usage-canvas: color-mix(in srgb, var(--surface-primary) 86%, var(--border-subtle) 14%);
  --usage-surface: var(--surface-raised);
  --usage-divider: var(--border-subtle);
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
  padding-block-end: 0.625rem;
  border-block-end: 1px solid var(--usage-divider);
  margin-block-end: 1rem;
}

.conversations__header h1 {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 680;
  letter-spacing: -0.025em;
  line-height: 1.15;
}

.conversations__back {
  min-inline-size: 3.25rem;
  min-block-size: 2.5rem;
  padding-inline: 0.75rem;
  border-radius: var(--radius-control);
  font-size: 0.75rem;
}

.conversations__filters {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
  margin-block-end: 0.875rem;
}

.conversations__search {
  display: flex;
  gap: 0.375rem;
}

.conversations__search input {
  inline-size: 16rem;
  min-block-size: 2.25rem;
  padding: 0 0.625rem;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-control);
  color: var(--text-primary);
  background: var(--usage-surface);
  font-size: 0.75rem;
}

.conversations__select select {
  min-block-size: 2.25rem;
  padding: 0 0.5rem;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-control);
  color: var(--text-primary);
  background: var(--usage-surface);
  font-size: 0.75rem;
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
  border: 1px solid var(--border-subtle);
  border-block-start-width: 0;
  color: inherit;
  background: var(--usage-surface);
  font-size: 0.75rem;
  text-align: start;
}

.conversations__list li:first-child .conversations__row {
  border-block-start-width: 1px;
  border-start-start-radius: 0.625rem;
  border-start-end-radius: 0.625rem;
}

.conversations__list li:last-child .conversations__row {
  border-end-start-radius: 0.625rem;
  border-end-end-radius: 0.625rem;
}

.conversations__row:hover {
  background: color-mix(in srgb, var(--usage-surface) 94%, var(--cat-codex) 6%);
}

.conversations__row[data-provider="claude"]:hover {
  background: color-mix(in srgb, var(--usage-surface) 94%, var(--cat-claude) 6%);
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

.conversations__main {
  display: flex;
  flex-direction: column;
  gap: 0.1875rem;
  min-inline-size: 0;
  flex: 1 1 auto;
}

.conversations__title {
  overflow: hidden;
  font-size: 0.78125rem;
  font-weight: 620;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conversations__meta {
  display: flex;
  gap: 0.625rem;
  color: var(--text-secondary);
  font-size: 0.625rem;
}

.conversations__stats {
  display: flex;
  align-items: baseline;
  gap: 1rem;
  flex: 0 0 auto;
  color: var(--text-secondary);
  font-size: 0.65625rem;
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

@media (max-width: 640px) {
  .conversations__stats {
    display: none;
  }

  .conversations__search input {
    inline-size: 10rem;
  }
}
</style>
