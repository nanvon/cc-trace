<script setup lang="ts">
/**
 * 用量构成面板：服务／提供商／模型／项目四个维度，替代原 By service／provider／model。
 *
 * 口径对齐 cc-bar `OverviewCompositionPanel`：每个维度全部列出，按排行口径降序，
 * 「其他」提供商、特殊项目与未归属固定排最后；行数多时在列表区内滚动。
 * 配色：服务用识别色，其余按排名套紫色色阶，「其他」灰，「未归属」斜纹（cc-bar §6.3）。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";

import type { RankingBasis } from "../features/settings/contracts";
import type { UsageSource, UsageSummary } from "../features/usage/contracts";
import {
  COMPOSITION_DIMENSIONS,
  buildModelRows,
  buildProjectRows,
  buildProviderRows,
  buildServiceRows,
  emptyTotals,
  rankShare,
  type CompositionColor,
  type CompositionDimension,
  type CompositionRow,
  type UsageTotals,
} from "../features/usage/overview";
import {
  formatUsageCost,
  presentUsageTokens,
  usageCacheHitRate,
} from "../features/usage/presentation";
import { usePrivacy } from "../lib/privacy";

const props = defineProps<{
  /** 源级汇总（已按可见服务过滤）。 */
  sourceSummary: UsageSummary | null;
  model: Record<UsageSource, UsageSummary | null>;
  projects: UsageSummary | null;
  sources: readonly UsageSource[];
  basis: RankingBasis;
  loaded: boolean;
  unavailable: boolean;
  /** 范围变化时用于收起已展开的提供商。 */
  scopeKey: string;
}>();

const emit = defineEmits<{
  "select-service": [source: UsageSource];
  "open-project": [key: string];
  "open-unattributed": [];
  "open-projects": [];
}>();

const { t, locale } = useI18n();
const privacy = usePrivacy();
const dimension = ref<CompositionDimension>("service");
const expandedProvider = ref<string | null>(null);

watch([dimension, () => props.scopeKey], () => {
  expandedProvider.value = null;
});

const ready = computed(() => props.loaded && !props.unavailable);

const rowsByDimension = computed<Record<CompositionDimension, CompositionRow[]>>(() => ({
  service: buildServiceRows(props.sourceSummary, props.sources, props.basis),
  provider: buildProviderRows(props.model, props.sources, props.basis),
  model: buildModelRows(props.model, props.sources, props.basis),
  project: buildProjectRows(props.projects, props.basis),
}));

const rows = computed(() => (ready.value ? rowsByDimension.value[dimension.value] : []));

/** 概览合计：占比与底部合计行的分母。 */
const total = computed<UsageTotals>(() => {
  const summary = props.sourceSummary;
  if (!summary) return emptyTotals();
  return {
    entryCount: summary.entryCount,
    requestCount: summary.requestCount,
    tokens: summary.tokens,
    fast: summary.fast,
    cost: summary.cost,
  };
});

const hasData = computed(() => rows.value.length > 0 && total.value.tokens.totalTokens > 0);

function share(row: CompositionRow): number {
  return rankShare(row.totals, total.value, props.basis);
}

const shareSegments = computed(() =>
  rows.value
    .map((row) => ({ id: row.id, color: row.color, share: share(row) }))
    .filter((segment) => segment.share > 0),
);

function tokenText(value: number): string {
  const display = presentUsageTokens(locale.value, value);
  const separator = display.unit && !locale.value.toLowerCase().startsWith("zh") ? " " : "";
  return `${display.value}${separator}${display.unit}`;
}

function costText(totals: UsageTotals): string {
  if (totals.entryCount === 0 && totals.tokens.totalTokens === 0) return t("main.noValue");
  return (
    formatUsageCost(locale.value, totals.cost, totals.entryCount, t("main.lessThanCent")) ??
    t("main.unpriced")
  );
}

function percentText(value: number): string {
  return new Intl.NumberFormat(locale.value, {
    maximumFractionDigits: 1,
    minimumFractionDigits: 1,
    style: "percent",
  }).format(value);
}

function hitRateText(totals: UsageTotals): string {
  const value = usageCacheHitRate(totals.tokens);
  return value === null ? t("main.noValue") : `${Math.round(value)}%`;
}

function serviceName(source: UsageSource): string {
  return t(`provider.${source}`);
}

function rowTitle(row: CompositionRow): string {
  if (row.kind === "unattributed") return t("overview.composition.unattributed");
  if (row.special === "none") return t("overview.composition.projectNone");
  if (row.special === "system") return t("overview.composition.projectSystem");
  if (row.special === "other") return t("overview.composition.rest");
  if (row.id.startsWith("service:")) return serviceName(row.sources[0] as UsageSource);
  if (row.id.startsWith("provider:")) return t(`overview.composition.provider.${row.title}`);
  // 项目名：隐私模式整体遮挡。
  if (row.id.startsWith("project:"))
    return privacy.enabled.value ? privacy.title(row.title) : row.title;
  return row.title;
}

function rowSubtitle(row: CompositionRow): string {
  if (row.kind === "unattributed") return t("overview.composition.unattributedHint");
  if (row.special === "none") return t("overview.composition.projectNoneHint");
  if (row.special === "system") return t("overview.composition.projectSystemHint");
  if (row.id.startsWith("service:")) {
    return t(`overview.composition.serviceSubtitle.${row.sources[0]}`);
  }
  if (row.subtitleIsPath) return privacy.path(row.subtitle);
  return row.sources.map(serviceName).join(" · ");
}

/** 悬停：该行的 Token 拆分（输入、输出、缓存命中、命中率）；有 Fast 用量时追加 Fast 明细。 */
function rowTooltip(row: CompositionRow): string {
  const tokens = row.totals.tokens;
  const parts = [
    `${t("overview.composition.tooltip.input")} ${tokenText(tokens.inputTokens)}`,
    `${t("overview.composition.tooltip.output")} ${tokenText(tokens.outputTokens)}`,
    `${t("overview.composition.tooltip.cacheHit")} ${tokenText(tokens.cacheReadInputTokens)}`,
    `${t("overview.composition.tooltip.hitRate")} ${hitRateText(row.totals)}`,
  ];
  const fast = row.totals.fast;
  if (fast.rawTokens > 0) {
    parts.push(
      `${t("overview.composition.tooltip.fast")} ${tokenText(fast.rawTokens)}`,
      `${t("overview.composition.tooltip.fastEquivalent")} ${tokenText(
        Number(fast.billingEquivalentTokens) || 0,
      )}`,
    );
  }
  return `${rowTitle(row)}\n${parts.join(" · ")}`;
}

function swatchStyle(color: CompositionColor): Record<string, string> {
  switch (color.kind) {
    case "service":
      return { "--swatch": `var(--cat-${color.source})` };
    case "rank":
      return { "--swatch": `var(--rank-${Math.min(color.index, 4) + 1})` };
    case "rest":
      return { "--swatch": "var(--rank-rest)" };
    default:
      return {};
  }
}

function swatchKind(color: CompositionColor): string {
  return color.kind === "unattributed" ? "unattributed" : "solid";
}

function isSecondary(row: CompositionRow): boolean {
  return row.kind !== "item" || row.color.kind === "rest";
}

function isInteractive(row: CompositionRow): boolean {
  return row.action.type !== "none";
}

function isExpanded(row: CompositionRow): boolean {
  return row.action.type === "provider" && expandedProvider.value === row.action.provider;
}

function perform(row: CompositionRow): void {
  const action = row.action;
  switch (action.type) {
    case "service":
      emit("select-service", action.source);
      break;
    case "provider":
      expandedProvider.value = expandedProvider.value === action.provider ? null : action.provider;
      break;
    case "project":
      emit("open-project", action.key);
      break;
    case "unattributed":
      emit("open-unattributed");
      break;
    default:
      break;
  }
}

const emptyMessage = computed(() => {
  if (props.unavailable) return t("main.unavailable");
  if (!props.loaded) return t("main.loading");
  if (dimension.value === "service" && props.sources.length === 0) {
    return t("overview.composition.noServices");
  }
  return t("overview.composition.empty");
});
</script>

<template>
  <section class="composition" aria-labelledby="composition-heading">
    <header class="composition__head">
      <h2 id="composition-heading">{{ t("overview.composition.title") }}</h2>
      <div
        class="composition__segmented"
        role="group"
        :aria-label="t('overview.composition.dimensionLabel')"
      >
        <button
          v-for="item in COMPOSITION_DIMENSIONS"
          :key="item"
          type="button"
          :aria-pressed="dimension === item"
          :data-selected="dimension === item ? 'true' : undefined"
          @click="dimension = item"
        >
          {{ t(`overview.composition.dimension.${item}`) }}
        </button>
      </div>
    </header>

    <p v-if="!hasData" class="composition__empty">{{ emptyMessage }}</p>
    <template v-else>
      <div class="composition__bar" role="img" :aria-label="t('overview.composition.title')">
        <i
          v-for="segment in shareSegments"
          :key="segment.id"
          class="composition__bar-segment"
          :data-kind="swatchKind(segment.color)"
          :style="{ flexGrow: segment.share, ...swatchStyle(segment.color) }"
        ></i>
      </div>

      <div class="composition__columns" aria-hidden="true">
        <span></span>
        <span>{{ t(`overview.composition.dimension.${dimension}`) }}</span>
        <span>{{ t("overview.composition.columns.tokens") }}</span>
        <span>{{ t("overview.composition.columns.cost") }}</span>
        <span>{{ t("overview.composition.columns.share") }}</span>
        <span>{{ t("overview.composition.columns.cacheHit") }}</span>
        <span></span>
      </div>

      <ul class="composition__list">
        <li v-for="row in rows" :key="row.id" class="composition__item">
          <component
            :is="isInteractive(row) ? 'button' : 'div'"
            class="composition__row"
            :class="{ 'composition__row--secondary': isSecondary(row) }"
            :type="isInteractive(row) ? 'button' : undefined"
            :title="rowTooltip(row)"
            :aria-expanded="row.action.type === 'provider' ? isExpanded(row) : undefined"
            @click="perform(row)"
          >
            <i
              class="composition__swatch"
              :data-kind="swatchKind(row.color)"
              :style="swatchStyle(row.color)"
              aria-hidden="true"
            ></i>
            <span class="composition__name">
              <strong>{{ rowTitle(row) }}</strong>
              <small v-if="rowSubtitle(row)">{{ rowSubtitle(row) }}</small>
            </span>
            <span class="composition__num composition__num--main numeric">{{
              tokenText(row.totals.tokens.totalTokens)
            }}</span>
            <span class="composition__num composition__num--main numeric">{{
              costText(row.totals)
            }}</span>
            <span class="composition__num numeric">{{ percentText(share(row)) }}</span>
            <span class="composition__num numeric">{{ hitRateText(row.totals) }}</span>
            <span class="composition__chevron" aria-hidden="true">
              <template v-if="row.action.type === 'provider'">{{
                isExpanded(row) ? "⌃" : "⌄"
              }}</template>
              <template
                v-else-if="row.action.type === 'project' || row.action.type === 'unattributed'"
                >›</template
              >
            </span>
          </component>

          <ul v-if="isExpanded(row)" class="composition__models">
            <li v-for="item in row.providerModels" :key="`${item.source}/${item.model}`">
              <i class="composition__mini" :style="{ '--swatch': `var(--cat-${item.source})` }"></i>
              <code>{{ item.model || t("main.noValue") }}</code>
              <span class="numeric">{{ tokenText(item.totals.tokens.totalTokens) }}</span>
              <span class="numeric">{{ costText(item.totals) }}</span>
            </li>
          </ul>
        </li>
      </ul>

      <footer class="composition__foot">
        <span class="numeric">{{
          t("overview.composition.total", {
            tokens: tokenText(total.tokens.totalTokens),
            cost: costText(total),
          })
        }}</span>
        <button
          v-if="dimension === 'project'"
          type="button"
          class="composition__link"
          @click="emit('open-projects')"
        >
          {{ t("overview.composition.allProjects") }}
        </button>
      </footer>
    </template>
  </section>
</template>

<style scoped>
.composition {
  container-type: inline-size;
  display: flex;
  flex-direction: column;
  gap: 0.625rem;
  min-inline-size: 0;
  padding: 1rem 1.125rem;
  background: var(--surface-raised);
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
}

.composition__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  flex-wrap: wrap;
}

.composition__head h2 {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.8125rem;
  font-weight: 600;
}

.composition__segmented {
  display: inline-flex;
  padding: 0.125rem;
  border-radius: var(--radius-control);
  background: var(--track-background);
}

.composition__segmented button {
  min-block-size: 1.75rem;
  padding: 0 0.625rem;
  border: 0;
  border-radius: calc(var(--radius-control) - 0.125rem);
  color: var(--text-secondary);
  background: transparent;
  font-size: 0.75rem;
  white-space: nowrap;
}

.composition__segmented button:hover {
  color: var(--text-primary);
}

.composition__segmented button[data-selected="true"] {
  color: var(--text-primary);
  background: var(--surface-raised);
  box-shadow: 0 1px 2px rgb(24 24 27 / 10%);
  font-weight: 570;
}

.composition__empty {
  display: grid;
  min-block-size: 7.5rem;
  margin: 0;
  place-items: center;
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

/* 占比条：6px 高，按行依次拼接，段间 2px 间隔 */
.composition__bar {
  display: flex;
  gap: 2px;
  block-size: 6px;
  overflow: hidden;
  border-radius: 999px;
}

.composition__bar-segment {
  display: block;
  min-inline-size: 2px;
  flex-basis: 0;
  background: var(--swatch);
}

.composition__bar-segment[data-kind="unattributed"],
.composition__swatch[data-kind="unattributed"] {
  background: repeating-linear-gradient(
    135deg,
    var(--rank-unattributed) 0 1px,
    transparent 1px 3px
  );
  box-shadow: inset 0 0 0 0.5px var(--rank-unattributed);
}

.composition__columns,
.composition__row {
  display: grid;
  grid-template-columns: 0.5rem minmax(0, 1fr) 5.25rem 5.25rem 3.25rem 4rem 0.75rem;
  align-items: center;
  column-gap: 0.5rem;
}

.composition__columns {
  color: var(--text-secondary);
  font-size: 0.65625rem;
  opacity: 0.8;
}

.composition__columns span:nth-child(n + 3) {
  text-align: end;
}

.composition__list {
  max-block-size: 15rem;
  padding: 0;
  margin: 0;
  overflow-y: auto;
  list-style: none;
}

.composition__item + .composition__item {
  border-block-start: 1px solid var(--border-hairline);
}

.composition__row {
  inline-size: 100%;
  padding: 0.375rem 0;
  border: 0;
  color: var(--text-primary);
  background: transparent;
  font: inherit;
  text-align: start;
}

button.composition__row {
  cursor: pointer;
}

button.composition__row:hover .composition__name strong {
  text-decoration: underline;
  text-decoration-color: var(--border-subtle);
  text-underline-offset: 3px;
}

.composition__swatch,
.composition__mini {
  display: block;
  inline-size: 0.5rem;
  block-size: 0.5rem;
  border-radius: 0.125rem;
  background: var(--swatch, var(--rank-rest));
}

.composition__name {
  display: grid;
  min-inline-size: 0;
  gap: 1px;
}

.composition__name strong {
  overflow: hidden;
  font-size: 0.78125rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composition__row--secondary .composition__name strong {
  color: var(--text-secondary);
  font-weight: 400;
}

.composition__name small {
  overflow: hidden;
  color: var(--text-secondary);
  font-size: 0.65625rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composition__num {
  color: var(--text-secondary);
  font-size: 0.6875rem;
  text-align: end;
  white-space: nowrap;
}

.composition__num--main {
  color: var(--text-primary);
  font-size: 0.8125rem;
  font-weight: 600;
}

.composition__chevron {
  color: var(--text-secondary);
  font-size: 0.75rem;
  text-align: center;
}

.composition__models {
  display: grid;
  gap: 0.375rem;
  padding: 0 0 0.5rem 1rem;
  margin: 0;
  list-style: none;
}

.composition__models li {
  display: grid;
  grid-template-columns: 0.5rem minmax(0, 1fr) 5.25rem 5.25rem;
  align-items: center;
  column-gap: 0.5rem;
  font-size: 0.6875rem;
}

.composition__models code {
  overflow: hidden;
  font-size: 0.71875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composition__models .numeric {
  color: var(--text-secondary);
  text-align: end;
}

.composition__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  padding-block-start: 0.625rem;
  border-block-start: 1px solid var(--border-hairline);
  color: var(--text-secondary);
  font-size: 0.6875rem;
}

.composition__link {
  padding: 0;
  border: 0;
  color: var(--action-primary);
  background: transparent;
  font: inherit;
  cursor: pointer;
}

.composition__link:hover {
  text-decoration: underline;
}

@container (max-width: 34rem) {
  .composition__columns,
  .composition__row {
    grid-template-columns: 0.5rem minmax(0, 1fr) 4.5rem 4.5rem 3rem 0.75rem;
  }

  .composition__columns span:nth-child(6),
  .composition__row .composition__num:nth-of-type(5) {
    display: none;
  }
}
</style>
