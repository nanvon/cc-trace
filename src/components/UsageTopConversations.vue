<script setup lang="ts">
/**
 * 高消耗对话：范围内按排行口径取前 5（cc-bar `OverviewTopConversationsPanel`）。
 * 每行：序号、服务小色块、标题、`项目 · 分支 · 日期`、金额条（按第一名归一，中性灰）、
 * 主数字（按口径）与 `另一项 · 占比`。点击行跳到对话页并选中该对话。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

import type { RankingBasis } from "../features/settings/contracts";
import type { UsageConversation, UsageSource } from "../features/usage/contracts";
import {
  PROJECT_KEY_NONE,
  PROJECT_KEY_SYSTEM,
  conversationTotals,
  emptyTotals,
  projectNameFromKey,
  rankRatio,
  rankShare,
  rankValue,
  topConversationsShare,
  type UsageTotals,
} from "../features/usage/overview";
import { formatUsageCost, presentUsageTokens } from "../features/usage/presentation";
import { usePrivacy } from "../lib/privacy";

const props = defineProps<{
  conversations: readonly UsageConversation[];
  /** 概览合计（占比分母）。 */
  total: UsageTotals | null;
  basis: RankingBasis;
  loaded: boolean;
  unavailable: boolean;
}>();

const emit = defineEmits<{
  open: [conversationKey: string];
  "open-all": [];
}>();

const { t, locale } = useI18n();
const privacy = usePrivacy();

const denominator = computed(() => props.total ?? emptyTotals());
const first = computed(() => props.conversations[0]);
const groupShare = computed(() =>
  topConversationsShare(props.conversations, props.total, props.basis),
);

function percentText(value: number): string {
  return new Intl.NumberFormat(locale.value, {
    maximumFractionDigits: 1,
    minimumFractionDigits: 1,
    style: "percent",
  }).format(value);
}

function tokenText(value: number): string {
  const display = presentUsageTokens(locale.value, value);
  const separator = display.unit && !locale.value.toLowerCase().startsWith("zh") ? " " : "";
  return `${display.value}${separator}${display.unit}`;
}

function costText(conversation: UsageConversation): string {
  return (
    formatUsageCost(
      locale.value,
      conversation.cost,
      conversation.entryCount,
      t("main.lessThanCent"),
    ) ?? t("overview.top.unpriced")
  );
}

function projectName(conversation: UsageConversation): string {
  const key = conversation.projectKey;
  if (key === PROJECT_KEY_NONE) return t("overview.composition.projectNone");
  if (key === PROJECT_KEY_SYSTEM) return t("overview.composition.projectSystem");
  const name = key ? projectNameFromKey(key) : (conversation.projectHint ?? "");
  return privacy.enabled.value ? privacy.title(name) : name;
}

function dayText(value: string): string {
  return new Intl.DateTimeFormat(locale.value, {
    day: "numeric",
    month: "numeric",
    year: "numeric",
  }).format(new Date(value));
}

function metaLine(conversation: UsageConversation): string {
  const parts = [projectName(conversation)];
  const branch = privacy.branch(conversation.branch);
  if (branch) parts.push(branch);
  parts.push(dayText(conversation.lastAt));
  return parts.filter(Boolean).join(" · ");
}

function titleText(conversation: UsageConversation): string {
  return privacy.enabled.value
    ? privacy.title(conversation.title ?? t("overview.top.untitled"))
    : (conversation.title ?? t("overview.top.untitled"));
}

function barRatio(conversation: UsageConversation): number {
  if (!first.value) return 0;
  return rankRatio(
    rankValue(conversationTotals(conversation), props.basis),
    rankValue(conversationTotals(first.value), props.basis),
  );
}

function shareOf(conversation: UsageConversation): number {
  return rankShare(conversationTotals(conversation), denominator.value, props.basis);
}

function primaryText(conversation: UsageConversation): string {
  return props.basis === "tokens"
    ? tokenText(conversation.tokens.totalTokens)
    : costText(conversation);
}

function secondaryText(conversation: UsageConversation): string {
  const other =
    props.basis === "tokens" ? costText(conversation) : tokenText(conversation.tokens.totalTokens);
  return `${other} · ${percentText(shareOf(conversation))}`;
}

const shareLabel = computed(() => {
  if (groupShare.value === null) return "";
  return t(props.basis === "tokens" ? "overview.top.shareTokens" : "overview.top.shareCost", {
    count: props.conversations.length,
    share: percentText(groupShare.value),
  });
});

const emptyMessage = computed(() => {
  if (props.unavailable) return t("main.unavailable");
  if (!props.loaded) return t("main.loading");
  return t("overview.top.empty");
});

function tileStyle(source: UsageSource): Record<string, string> {
  return { "--tile": `var(--cat-${source})` };
}
</script>

<template>
  <section class="top" aria-labelledby="top-conversations-heading">
    <header class="top__head">
      <h2 id="top-conversations-heading">{{ t("overview.top.title") }}</h2>
      <div class="top__tools">
        <span v-if="shareLabel" class="top__share numeric">{{ shareLabel }}</span>
        <button type="button" class="top__link" @click="emit('open-all')">
          {{ t("overview.top.all") }}
        </button>
      </div>
    </header>

    <p v-if="conversations.length === 0" class="top__empty">{{ emptyMessage }}</p>
    <ol v-else class="top__list">
      <li v-for="(conversation, index) in conversations" :key="conversation.conversationKey">
        <button type="button" class="top__row" @click="emit('open', conversation.conversationKey)">
          <span class="top__index numeric">{{ index + 1 }}</span>
          <i class="top__tile" :style="tileStyle(conversation.source)" aria-hidden="true"></i>
          <span class="top__main">
            <strong>{{ titleText(conversation) }}</strong>
            <small>{{ metaLine(conversation) }}</small>
            <span class="top__track" aria-hidden="true">
              <i :style="{ inlineSize: `${barRatio(conversation) * 100}%` }"></i>
            </span>
          </span>
          <span class="top__value">
            <strong class="numeric">{{ primaryText(conversation) }}</strong>
            <small class="numeric">{{ secondaryText(conversation) }}</small>
          </span>
        </button>
      </li>
    </ol>
  </section>
</template>

<style scoped>
.top {
  display: flex;
  flex-direction: column;
  gap: 0.625rem;
  min-inline-size: 0;
  padding: 1rem 1.125rem;
  background: var(--surface-raised);
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
}

.top__head,
.top__tools {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.top__head {
  justify-content: space-between;
  flex-wrap: wrap;
}

.top__head h2 {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.8125rem;
  font-weight: 600;
}

.top__share {
  color: var(--text-secondary);
  font-size: 0.65625rem;
  opacity: 0.85;
}

.top__link {
  padding: 0;
  border: 0;
  color: var(--action-primary);
  background: transparent;
  font: inherit;
  font-size: 0.6875rem;
  cursor: pointer;
}

.top__link:hover {
  text-decoration: underline;
}

.top__empty {
  display: grid;
  min-block-size: 7.5rem;
  margin: 0;
  place-items: center;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

.top__list {
  padding: 0;
  margin: 0;
  list-style: none;
}

.top__list li + li {
  border-block-start: 1px solid var(--border-hairline);
}

.top__row {
  display: grid;
  grid-template-columns: 0.875rem 0.625rem minmax(0, 1fr) auto;
  align-items: center;
  column-gap: 0.5rem;
  inline-size: 100%;
  padding: 0.6875rem 0;
  border: 0;
  color: var(--text-primary);
  background: transparent;
  font: inherit;
  text-align: start;
  cursor: pointer;
}

.top__row:hover .top__main strong {
  text-decoration: underline;
  text-decoration-color: var(--border-subtle);
  text-underline-offset: 3px;
}

.top__index {
  color: var(--text-secondary);
  font-family: var(--font-mono, ui-monospace, monospace);
  font-size: 0.6875rem;
  text-align: end;
}

.top__tile {
  display: block;
  inline-size: 0.625rem;
  block-size: 0.625rem;
  border-radius: 0.1875rem;
  background: var(--tile);
}

.top__main {
  display: grid;
  min-inline-size: 0;
  gap: 2px;
}

.top__main strong {
  overflow: hidden;
  font-size: 0.78125rem;
  font-weight: 550;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.top__main small {
  overflow: hidden;
  color: var(--text-secondary);
  font-size: 0.65625rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 金额条：铺满文字列，按第一名归一，中性灰 */
.top__track {
  display: block;
  block-size: 3px;
  margin-block-start: 0.25rem;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in srgb, var(--text-secondary) 14%, transparent);
}

.top__track i {
  display: block;
  block-size: 100%;
  border-radius: 999px;
  background: var(--amount-bar);
}

.top__value {
  display: grid;
  min-inline-size: 6rem;
  gap: 2px;
  text-align: end;
}

.top__value strong {
  font-size: 0.78125rem;
  font-weight: 600;
}

.top__value small {
  color: var(--text-secondary);
  font-size: 0.65625rem;
}
</style>
