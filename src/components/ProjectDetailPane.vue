<script setup lang="ts">
/**
 * 项目页右侧详情面板（对齐 cc-bar 项目分析页）。
 *
 * 范围与服务过滤跟随全局共享状态（用量页／对话页同一份）。详情包含：标题与路径、
 * KPI、全部时间一行、每日用量、工具与模型、分支表、高消耗对话前 3、仓库与 worktree；
 * 未归属分组只展示按来源拆分的说明与汇总，不画图表也不列对话（它们没有对话身份）。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";

import UsageDailyChart from "./UsageDailyChart.vue";
import UsageModelTable from "./UsageModelTable.vue";
import {
  getProjectBreakdown,
  getUsageSummary,
  listConversations,
  revealProject,
} from "../features/usage/api";
import type {
  UsageConversation,
  UsageCostTotals,
  UsageFilter,
  UsageProjectBreakdown,
  UsageProjectSummary,
  UsageSource,
  UsageSummary,
  UsageTokenTotals,
} from "../features/usage/contracts";
import { formatUsageCost, presentUsageTokens } from "../features/usage/presentation";
import { usageChartRange } from "../features/usage/ranges";
import { useUsageStore } from "../features/usage/store";
import { usePrivacy } from "../lib/privacy";

const props = defineProps<{
  project: UsageProjectSummary | null;
  /** 列表刷新计数：扫描完成后递增，触发详情重载。 */
  refreshToken?: number;
}>();

const { t, locale } = useI18n();
const router = useRouter();
const usage = useUsageStore();
const privacy = usePrivacy();

const KEY_NONE = "@none";
const KEY_SYSTEM = "@system";

const loading = ref(false);
const unavailable = ref(false);
const breakdown = ref<UsageProjectBreakdown | null>(null);
const sourceSummary = ref<UsageSummary | null>(null);
const daySummaries = ref<Record<UsageSource, UsageSummary | null>>(emptySources());
const modelSummaries = ref<Record<UsageSource, UsageSummary | null>>(emptySources());
const topConversations = ref<UsageConversation[]>([]);
const revealFailed = ref(false);
let request = 0;

function emptySources(): Record<UsageSource, UsageSummary | null> {
  return { codex: null, claude: null, pi: null, opencode: null, dsh: null, cursor: null };
}

const isUnattributed = computed(() => props.project?.unattributed === true);
const isSpecial = computed(
  () => props.project?.key === KEY_NONE || props.project?.key === KEY_SYSTEM,
);
const sources = computed(() => usage.dashboardSources);
const chartRange = computed(() => usageChartRange(usage.dashboardRange, usage.granularity));

const title = computed(() => {
  const project = props.project;
  if (!project) return "";
  if (project.unattributed) return t("projects.special.unattributed");
  if (project.key === KEY_NONE) return t("projects.special.none");
  if (project.key === KEY_SYSTEM) return t("projects.special.system");
  return project.name;
});

const subtitle = computed(() => {
  const project = props.project;
  if (!project) return "";
  if (project.unattributed) return t("projects.special.unattributedHint");
  if (project.key === KEY_NONE) return t("projects.special.noneHint");
  if (project.key === KEY_SYSTEM) return t("projects.special.systemHint");
  return privacy.path(project.path);
});

const canReveal = computed(
  () => !!props.project && !props.project.unattributed && !isSpecial.value && !!props.project.path,
);

function filterFor(withRange: "range" | "chart", source: UsageSource | null): UsageFilter {
  const range = withRange === "chart" ? chartRange.value : usage.dashboardRange;
  return {
    from: range.from,
    to: range.to,
    sources: source === null ? [...sources.value] : [source],
    model: null,
    speed: null,
    project: props.project?.key ?? null,
  };
}

async function load(): Promise<void> {
  const project = props.project;
  const current = ++request;
  revealFailed.value = false;
  if (!project) {
    breakdown.value = null;
    return;
  }
  loading.value = true;
  unavailable.value = false;

  const rangeFilter = filterFor("range", null);
  const queries: Array<Promise<unknown>> = [
    getProjectBreakdown({ key: project.key, filter: rangeFilter }),
  ];
  // 未归属分组没有项目身份：不能按项目过滤，也就不画图表与模型表。
  const withCharts = !project.unattributed;
  if (withCharts) {
    queries.push(
      getUsageSummary({ filter: rangeFilter, groupBy: "source" }),
      listConversations({
        filter: rangeFilter,
        search: null,
        project: project.key,
        sort: "cost",
        limit: 3,
        offset: 0,
      }),
      ...sources.value.map((source) =>
        getUsageSummary({ filter: filterFor("chart", source), groupBy: "day" }),
      ),
      ...sources.value.map((source) =>
        getUsageSummary({ filter: filterFor("range", source), groupBy: "model" }),
      ),
    );
  }
  const results = await Promise.allSettled(queries);
  if (current !== request) return;

  const value = <T,>(index: number): T | null => {
    const result = results[index];
    return result && result.status === "fulfilled" ? (result.value as T) : null;
  };
  breakdown.value = value<UsageProjectBreakdown>(0);
  unavailable.value = results.some((result) => result.status === "rejected");
  if (withCharts) {
    sourceSummary.value = value<UsageSummary>(1);
    topConversations.value = value<{ items: UsageConversation[] }>(2)?.items ?? [];
    const day = emptySources();
    const model = emptySources();
    sources.value.forEach((source, index) => {
      day[source] = value<UsageSummary>(3 + index);
      model[source] = value<UsageSummary>(3 + sources.value.length + index);
    });
    daySummaries.value = day;
    modelSummaries.value = model;
  } else {
    sourceSummary.value = null;
    topConversations.value = [];
    daySummaries.value = emptySources();
    modelSummaries.value = emptySources();
  }
  loading.value = false;
}

watch(
  () => [
    props.project?.key,
    props.refreshToken,
    usage.dashboardRange,
    usage.granularity,
    usage.sourceFilter,
    usage.visibleSources.join(","),
  ],
  () => void load(),
  { immediate: true },
);

async function reveal(): Promise<void> {
  const path = props.project?.path;
  if (!path) return;
  revealFailed.value = false;
  try {
    revealFailed.value = !(await revealProject(path));
  } catch {
    revealFailed.value = true;
  }
}

function openConversations(): void {
  if (!props.project) return;
  void router.push({ name: "conversations", query: { project: props.project.key } });
}

function tokensText(tokens: UsageTokenTotals): string {
  const display = presentUsageTokens(locale.value, tokens.totalTokens);
  return `${display.value}${display.unit}`;
}

function costText(cost: UsageCostTotals): string {
  return (
    formatUsageCost(
      locale.value,
      cost,
      cost.pricedEntries + cost.unpricedEntries,
      t("main.lessThanCent"),
    ) ?? t("main.unpriced")
  );
}

function dayText(value: string | null): string {
  if (!value) return t("main.noValue");
  const date = /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(`${value}T00:00:00`) : new Date(value);
  return new Intl.DateTimeFormat(locale.value, { dateStyle: "medium" }).format(date);
}

const summary = computed(() => props.project);
const rangeTokens = computed(() => sourceSummary.value?.tokens ?? summary.value?.tokens ?? null);
const rangeCost = computed(() => sourceSummary.value?.cost ?? summary.value?.cost ?? null);

const kpis = computed(() => {
  const project = summary.value;
  if (!project) return [];
  return [
    {
      key: "tokens",
      label: t("projects.kpi.tokens"),
      text: rangeTokens.value ? tokensText(rangeTokens.value) : t("main.noValue"),
    },
    {
      key: "cost",
      label: t("projects.kpi.cost"),
      text: rangeCost.value ? costText(rangeCost.value) : t("main.noValue"),
    },
    {
      key: "conversations",
      label: t("projects.kpi.conversations"),
      text: String(project.conversationCount),
    },
    { key: "days", label: t("projects.kpi.activeDays"), text: String(project.activeDays) },
  ];
});

const allTimeText = computed(() => {
  const all = breakdown.value?.allTime;
  if (!all) return "";
  return t("projects.allTimeLine", {
    tokens: tokensText(all.tokens),
    cost: costText(all.cost),
    first: dayText(all.firstAt),
  });
});

const repoKind = computed(() => {
  const project = summary.value;
  if (!project) return null;
  if (project.status === "unverified") return "unverified";
  if (project.status === "unavailable") return "missing";
  return project.isGit ? "git" : "plain";
});

function sourceLabel(row: { source: UsageSource; granularity: string }): string {
  const kind =
    row.granularity === "day"
      ? t("projects.unattributed.remote")
      : t("projects.unattributed.backfill");
  return `${t(`provider.${row.source}`)} · ${kind}`;
}
</script>

<template>
  <div class="project-detail">
    <p v-if="!project" class="project-detail__empty">{{ t("projects.selectHint") }}</p>

    <template v-else>
      <header class="project-detail__head">
        <div class="project-detail__title">
          <h2>{{ title }}</h2>
          <p
            v-if="subtitle"
            class="project-detail__path"
            :title="isSpecial || isUnattributed ? undefined : subtitle"
          >
            {{ subtitle }}
          </p>
        </div>
        <button v-if="canReveal" type="button" class="button button--quiet" @click="reveal">
          {{ t("projects.reveal") }}
        </button>
      </header>
      <p v-if="revealFailed" class="project-detail__warn" role="status">
        {{ t("projects.revealFailed") }}
      </p>
      <p v-if="unavailable" class="project-detail__warn" role="status">
        {{ t("projects.unavailable") }}
      </p>

      <section class="project-detail__kpi" role="group" :aria-label="t('projects.kpi.label')">
        <div v-for="card in kpis" :key="card.key" class="project-detail__kpi-card">
          <span>{{ card.label }}</span>
          <strong class="numeric">{{ card.text }}</strong>
        </div>
      </section>

      <p v-if="breakdown" class="project-detail__alltime">
        <span>{{ t("projects.allTime") }}</span>
        {{ allTimeText }}
      </p>

      <!-- 未归属分组：按来源拆分 -->
      <section v-if="isUnattributed" class="project-detail__block">
        <h3>{{ t("projects.unattributed.title") }}</h3>
        <p class="project-detail__note">{{ t("projects.unattributed.note") }}</p>
        <table
          v-if="breakdown && breakdown.unattributedSources.length > 0"
          class="project-detail__table"
        >
          <thead>
            <tr>
              <th scope="col">{{ t("projects.unattributed.source") }}</th>
              <th scope="col">{{ t("projects.unattributed.period") }}</th>
              <th scope="col" class="num">{{ t("projects.col.tokens") }}</th>
              <th scope="col" class="num">{{ t("projects.col.cost") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="row in breakdown.unattributedSources"
              :key="`${row.source}-${row.granularity}`"
            >
              <td>{{ sourceLabel(row) }}</td>
              <td>{{ dayText(row.firstDay) }} – {{ dayText(row.lastDay) }}</td>
              <td class="num numeric">{{ tokensText(row.tokens) }}</td>
              <td class="num numeric">{{ costText(row.cost) }}</td>
            </tr>
          </tbody>
        </table>
      </section>

      <template v-else>
        <section class="project-detail__block">
          <h3>{{ t("projects.daily") }}</h3>
          <UsageDailyChart
            :day="daySummaries"
            :sources="sources"
            :range="usage.dashboardRange"
            :chart-range="chartRange"
            :granularity="usage.granularity"
            :loaded="!loading"
            :unavailable="unavailable"
          />
        </section>

        <section class="project-detail__block">
          <h3>{{ t("projects.byModel") }}</h3>
          <UsageModelTable
            :model="modelSummaries"
            :sources="sources"
            :source-summary="sourceSummary"
            :loaded="!loading"
            :unavailable="unavailable"
          />
        </section>

        <section v-if="breakdown && breakdown.branches.length > 0" class="project-detail__block">
          <h3>{{ t("projects.branches") }}</h3>
          <table class="project-detail__table">
            <thead>
              <tr>
                <th scope="col">{{ t("projects.col.branch") }}</th>
                <th scope="col" class="num">{{ t("projects.col.conversations") }}</th>
                <th scope="col" class="num">{{ t("projects.col.tokens") }}</th>
                <th scope="col" class="num">{{ t("projects.col.cost") }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="row in breakdown.branches" :key="row.branch">
                <td>{{ row.branch ? privacy.branch(row.branch) : t("projects.noBranch") }}</td>
                <td class="num numeric">{{ row.conversationCount }}</td>
                <td class="num numeric">{{ tokensText(row.tokens) }}</td>
                <td class="num numeric">{{ costText(row.cost) }}</td>
              </tr>
            </tbody>
          </table>
        </section>

        <section v-if="topConversations.length > 0" class="project-detail__block">
          <div class="project-detail__block-head">
            <h3>{{ t("projects.topConversations") }}</h3>
            <button type="button" class="button button--quiet" @click="openConversations">
              {{ t("projects.viewConversations") }}
            </button>
          </div>
          <ul class="project-detail__top">
            <li v-for="conversation in topConversations" :key="conversation.conversationKey">
              <span class="project-detail__top-title">
                {{ privacy.title(conversation.title) || t("conversations.untitled") }}
              </span>
              <span class="numeric">{{ tokensText(conversation.tokens) }}</span>
              <strong class="numeric">{{ costText(conversation.cost) }}</strong>
            </li>
          </ul>
        </section>

        <section v-if="!isSpecial" class="project-detail__block">
          <h3>{{ t("projects.repo") }}</h3>
          <p class="project-detail__note">
            {{ t(`projects.repoKind.${repoKind ?? "plain"}`) }}
          </p>
          <table v-if="breakdown && breakdown.worktrees.length > 0" class="project-detail__table">
            <thead>
              <tr>
                <th scope="col">{{ t("projects.col.worktree") }}</th>
                <th scope="col">{{ t("projects.col.branch") }}</th>
                <th scope="col" class="num">{{ t("projects.col.tokens") }}</th>
                <th scope="col" class="num">{{ t("projects.col.cost") }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="row in breakdown.worktrees" :key="row.path">
                <td class="project-detail__cell-path" :title="privacy.path(row.path)">
                  {{ privacy.path(row.path) }}
                  <em v-if="row.path === project.key">{{ t("projects.mainWorktree") }}</em>
                </td>
                <td>{{ row.branch ? privacy.branch(row.branch) : t("projects.noBranch") }}</td>
                <td class="num numeric">{{ tokensText(row.tokens) }}</td>
                <td class="num numeric">{{ costText(row.cost) }}</td>
              </tr>
            </tbody>
          </table>
        </section>
      </template>
    </template>
  </div>
</template>

<style scoped>
.project-detail {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding-inline-start: 1.25rem;
  font-family: var(--font-ui);
}

.project-detail__empty {
  padding: 2.5rem 1rem;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

.project-detail__head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
}

.project-detail__title {
  min-inline-size: 0;
}

.project-detail__title h2 {
  margin: 0;
  font-size: 1.125rem;
  font-weight: 700;
  overflow-wrap: anywhere;
}

.project-detail__path {
  margin: 0.25rem 0 0;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  overflow-wrap: anywhere;
}

.project-detail__warn {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.project-detail__kpi {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 0.5rem;
}

.project-detail__kpi-card {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  padding: 0.625rem 0.75rem;
  border: 1px solid var(--border-hairline);
  border-radius: 0.75rem;
  background: var(--surface-raised);
}

.project-detail__kpi-card span {
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.project-detail__kpi-card strong {
  font-size: 1rem;
  font-variant-numeric: tabular-nums;
}

.project-detail__alltime {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.project-detail__alltime span {
  margin-inline-end: 0.5rem;
  color: var(--text-primary);
  font-weight: 600;
}

.project-detail__block {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  min-inline-size: 0;
}

.project-detail__block h3 {
  margin: 0;
  font-size: 0.8125rem;
  font-weight: 600;
}

.project-detail__block-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
}

.project-detail__note {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.project-detail__table {
  inline-size: 100%;
  border-collapse: collapse;
  font-size: 0.6875rem;
}

.project-detail__table th,
.project-detail__table td {
  padding: 0.375rem 0.5rem;
  border-block-end: 1px solid var(--border-hairline);
  text-align: start;
}

.project-detail__table th {
  color: var(--text-secondary);
  font-weight: 600;
}

.project-detail__table .num {
  text-align: end;
  font-variant-numeric: tabular-nums;
}

.project-detail__cell-path {
  overflow-wrap: anywhere;
}

.project-detail__cell-path em {
  margin-inline-start: 0.375rem;
  color: var(--text-secondary);
  font-style: normal;
}

.project-detail__top {
  margin: 0;
  padding: 0;
  list-style: none;
}

.project-detail__top li {
  display: flex;
  align-items: baseline;
  gap: 1rem;
  padding: 0.375rem 0;
  border-block-end: 1px solid var(--border-hairline);
  font-size: 0.75rem;
}

.project-detail__top-title {
  min-inline-size: 0;
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@container (max-width: 640px) {
  .project-detail__kpi {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
