/**
 * 概览页的纯派生逻辑：用量构成四个维度、周期归并、高消耗对话口径。
 * 口径对齐 cc-bar `StatsOverviewModel`：四个维度都全部列出，按排行口径降序，
 * 「其他」提供商、特殊项目与未归属固定在最后，占比以概览合计为分母。
 */
import type { RankingBasis } from "../settings/contracts";
import { USAGE_SOURCES } from "./contracts";
import type {
  UsageConversation,
  UsageCostTotals,
  UsageFastTotals,
  UsageGranularity,
  UsageSource,
  UsageSummary,
  UsageSummaryRow,
  UsageTokenTotals,
} from "./contracts";
import { usageBucketStart, usageDayKey, usageParseDayKey } from "./ranges";

export type CompositionDimension = "service" | "provider" | "model" | "project";
export const COMPOSITION_DIMENSIONS: readonly CompositionDimension[] = [
  "service",
  "provider",
  "model",
  "project",
] as const;

/** 项目保留键：与后端约定，见 ADR-0034。 */
export const PROJECT_KEY_UNATTRIBUTED = "";
export const PROJECT_KEY_NONE = "@none";
export const PROJECT_KEY_SYSTEM = "@system";

export type ModelProvider =
  "openai" | "anthropic" | "deepseek" | "opencodeGo" | "commandCode" | "other";

/** 一行的汇总数值，与 `UsageSummaryRow` 的数值字段一致。 */
export type UsageTotals = Pick<
  UsageSummaryRow,
  "entryCount" | "requestCount" | "tokens" | "fast" | "cost"
>;

export type CompositionColor =
  | { kind: "service"; source: UsageSource }
  | { kind: "rank"; index: number }
  | { kind: "rest" }
  | { kind: "unattributed" };

export type CompositionAction =
  | { type: "none" }
  | { type: "service"; source: UsageSource }
  | { type: "provider"; provider: ModelProvider }
  | { type: "project"; key: string }
  | { type: "unattributed" };

export interface ProviderModelRow {
  source: UsageSource;
  model: string;
  totals: UsageTotals;
}

export interface CompositionRow {
  id: string;
  /** `item` 为普通行；`unattributed` 的名称与说明由视图按 i18n 生成。 */
  kind: "item" | "unattributed";
  /** 特殊项目（无明确项目／系统任务）与「其他」提供商的名称由视图本地化。 */
  special: "none" | "system" | "other" | null;
  title: string;
  /** 项目行为路径尾段；其余维度为副标文本。 */
  subtitle: string;
  /** 副标是路径，需要隐私遮挡。 */
  subtitleIsPath: boolean;
  color: CompositionColor;
  totals: UsageTotals;
  action: CompositionAction;
  sources: UsageSource[];
  providerModels: ProviderModelRow[];
}

// ---------- 数值合并 ----------

export function emptyTokens(): UsageTokenTotals {
  return {
    uncachedInputTokens: 0,
    outputTokens: 0,
    reasoningOutputTokens: 0,
    cacheReadInputTokens: 0,
    cacheWrite5mInputTokens: 0,
    cacheWrite1hInputTokens: 0,
    inputTokens: 0,
    totalTokens: 0,
  };
}

export function emptyTotals(): UsageTotals {
  const fast: UsageFastTotals = {
    rawTokens: 0,
    billingEquivalentTokens: "0",
    minimumMultiplier: null,
    maximumMultiplier: null,
    hasUnpricedEquivalent: false,
  };
  const cost: UsageCostTotals = {
    apiEquivalentCostNanos: 0,
    pricedEntries: 0,
    unpricedEntries: 0,
    assumedGeoEntries: 0,
    pricingFingerprint: null,
  };
  return { entryCount: 0, requestCount: 0, tokens: emptyTokens(), fast, cost };
}

function pickMultiplier(
  left: string | null,
  right: string | null,
  pick: (a: number, b: number) => number,
): string | null {
  if (left === null) return right;
  if (right === null) return left;
  const a = Number(left);
  const b = Number(right);
  if (!Number.isFinite(a) || !Number.isFinite(b)) return left;
  return String(pick(a, b));
}

/** 累加一行到目标；返回目标本身。 */
export function addTotals(target: UsageTotals, row: UsageTotals): UsageTotals {
  target.entryCount += row.entryCount;
  target.requestCount += row.requestCount;
  const t = target.tokens;
  const r = row.tokens;
  t.uncachedInputTokens += r.uncachedInputTokens;
  t.outputTokens += r.outputTokens;
  t.reasoningOutputTokens += r.reasoningOutputTokens;
  t.cacheReadInputTokens += r.cacheReadInputTokens;
  t.cacheWrite5mInputTokens += r.cacheWrite5mInputTokens;
  t.cacheWrite1hInputTokens += r.cacheWrite1hInputTokens;
  t.inputTokens += r.inputTokens;
  t.totalTokens += r.totalTokens;
  const f = target.fast;
  f.rawTokens += row.fast.rawTokens;
  f.billingEquivalentTokens = String(
    (Number(f.billingEquivalentTokens) || 0) + (Number(row.fast.billingEquivalentTokens) || 0),
  );
  f.minimumMultiplier = pickMultiplier(f.minimumMultiplier, row.fast.minimumMultiplier, Math.min);
  f.maximumMultiplier = pickMultiplier(f.maximumMultiplier, row.fast.maximumMultiplier, Math.max);
  f.hasUnpricedEquivalent ||= row.fast.hasUnpricedEquivalent;
  const c = target.cost;
  c.apiEquivalentCostNanos += row.cost.apiEquivalentCostNanos;
  c.pricedEntries += row.cost.pricedEntries;
  c.unpricedEntries += row.cost.unpricedEntries;
  c.assumedGeoEntries += row.cost.assumedGeoEntries;
  c.pricingFingerprint ??= row.cost.pricingFingerprint;
  return target;
}

export function hasUsage(totals: UsageTotals): boolean {
  return totals.entryCount > 0 || totals.tokens.totalTokens > 0;
}

// ---------- 排行口径 ----------

export function rankValue(totals: UsageTotals, basis: RankingBasis): number {
  return basis === "cost" ? totals.cost.apiEquivalentCostNanos : totals.tokens.totalTokens;
}

/**
 * 占比（0~1）。按费用但合计无金额（全部未定价）时退回按 Tokens，与 cc-bar `share(of:in:)` 一致。
 */
export function rankShare(part: UsageTotals, total: UsageTotals, basis: RankingBasis): number {
  if (basis === "cost" && total.cost.apiEquivalentCostNanos > 0) {
    return part.cost.apiEquivalentCostNanos / total.cost.apiEquivalentCostNanos;
  }
  if (total.tokens.totalTokens <= 0) return 0;
  return part.tokens.totalTokens / total.tokens.totalTokens;
}

/** 条长比例：`value / reference`，reference 为 0 时为 0，上限 1。 */
export function rankRatio(value: number, reference: number): number {
  if (reference <= 0) return 0;
  return Math.min(1, Math.max(0, value / reference));
}

// ---------- 提供商推导 ----------

const PROVIDER_PREFIXES: ReadonlyArray<[string, ModelProvider]> = [
  ["openai-codex/", "openai"],
  ["openai/", "openai"],
  ["anthropic/", "anthropic"],
  ["deepseek/", "deepseek"],
  ["opencode-go/", "opencodeGo"],
  ["commandcode/", "commandCode"],
  ["command-code/", "commandCode"],
];

/** 提供商推导：前缀 → 模型名关键词 → 按服务兜底 → 其他（对齐 cc-bar `ModelProvider.resolve`）。 */
export function resolveModelProvider(source: UsageSource, model: string): ModelProvider {
  const m = model.toLowerCase();
  for (const [prefix, provider] of PROVIDER_PREFIXES) {
    if (m.startsWith(prefix)) return provider;
  }
  if (m.startsWith("claude-")) return "anthropic";
  if (
    m.startsWith("gpt-") ||
    m.startsWith("o1") ||
    m.startsWith("o3") ||
    m.startsWith("o4") ||
    m.startsWith("chatgpt-") ||
    m.startsWith("codex-")
  ) {
    return "openai";
  }
  if (m.startsWith("deepseek-")) return "deepseek";
  if (source === "codex") return "openai";
  if (source === "claude") return "anthropic";
  return "other";
}

// ---------- 构成行 ----------

function byRankThenName<T extends { totals: UsageTotals; name: string }>(
  items: T[],
  basis: RankingBasis,
): T[] {
  return [...items].sort((left, right) => {
    const l = rankValue(left.totals, basis);
    const r = rankValue(right.totals, basis);
    if (l === r) return left.name < right.name ? -1 : left.name > right.name ? 1 : 0;
    return r - l;
  });
}

function rowTotals(row: UsageSummaryRow): UsageTotals {
  return addTotals(emptyTotals(), row);
}

export function buildServiceRows(
  sourceSummary: UsageSummary | null,
  sources: readonly UsageSource[],
  basis: RankingBasis,
): CompositionRow[] {
  const rows = sources.map((source, order) => {
    const found = sourceSummary?.rows.find((row) => row.key === source);
    return { source, order, totals: found ? rowTotals(found) : emptyTotals(), name: source };
  });
  rows.sort((left, right) => {
    const l = rankValue(left.totals, basis);
    const r = rankValue(right.totals, basis);
    return l === r ? left.order - right.order : r - l;
  });
  return rows.map(({ source, totals }) => ({
    id: `service:${source}`,
    kind: "item",
    special: null,
    title: source,
    subtitle: "",
    subtitleIsPath: false,
    color: { kind: "service", source },
    totals,
    action: { type: "service", source },
    sources: [source],
    providerModels: [],
  }));
}

type ModelBySource = Partial<Record<UsageSource, UsageSummary | null>>;

export function buildProviderRows(
  model: ModelBySource,
  sources: readonly UsageSource[],
  basis: RankingBasis,
): CompositionRow[] {
  const groups = new Map<
    ModelProvider,
    { totals: UsageTotals; sources: Set<UsageSource>; models: ProviderModelRow[] }
  >();
  for (const source of sources) {
    for (const row of model[source]?.rows ?? []) {
      const provider = resolveModelProvider(source, row.key);
      const group = groups.get(provider) ?? {
        totals: emptyTotals(),
        sources: new Set<UsageSource>(),
        models: [],
      };
      addTotals(group.totals, row);
      group.sources.add(source);
      group.models.push({ source, model: row.key, totals: rowTotals(row) });
      groups.set(provider, group);
    }
  }
  const named = [...groups.entries()].map(([provider, group]) => ({
    provider,
    group,
    totals: group.totals,
    name: provider,
  }));
  const ranked = byRankThenName(
    named.filter((item) => item.provider !== "other"),
    basis,
  );
  const rest = named.filter((item) => item.provider === "other");
  return [...ranked, ...rest].map((item, index) => ({
    id: `provider:${item.provider}`,
    kind: "item",
    special: item.provider === "other" ? "other" : null,
    title: item.provider,
    subtitle: "",
    subtitleIsPath: false,
    color: item.provider === "other" ? { kind: "rest" } : { kind: "rank", index },
    totals: item.totals,
    action: { type: "provider", provider: item.provider },
    sources: USAGE_SOURCES.filter((source) => item.group.sources.has(source)),
    providerModels: byRankThenName(
      item.group.models.map((entry) => ({ ...entry, name: entry.model })),
      basis,
    ),
  }));
}

export function buildModelRows(
  model: ModelBySource,
  sources: readonly UsageSource[],
  basis: RankingBasis,
): CompositionRow[] {
  const merged = new Map<
    string,
    { totals: UsageTotals; sources: Set<UsageSource>; providers: Set<ModelProvider> }
  >();
  for (const source of sources) {
    for (const row of model[source]?.rows ?? []) {
      const item = merged.get(row.key) ?? {
        totals: emptyTotals(),
        sources: new Set<UsageSource>(),
        providers: new Set<ModelProvider>(),
      };
      addTotals(item.totals, row);
      item.sources.add(source);
      item.providers.add(resolveModelProvider(source, row.key));
      merged.set(row.key, item);
    }
  }
  const sorted = byRankThenName(
    [...merged.entries()].map(([name, item]) => ({ name, item, totals: item.totals })),
    basis,
  );
  return sorted.map(({ name, item }, index) => ({
    id: `model:${name}`,
    kind: "item",
    special: null,
    title: name || "—",
    subtitle: "",
    subtitleIsPath: false,
    color: { kind: "rank", index },
    totals: item.totals,
    action: { type: "none" },
    sources: USAGE_SOURCES.filter((source) => item.sources.has(source)),
    providerModels: [],
  }));
}

/** 项目名：路径最后一段（兼容 `/` 与 `\`）。 */
export function projectNameFromKey(key: string): string {
  const parts = key
    .replace(/[\\/]+$/, "")
    .split(/[\\/]/)
    .filter(Boolean);
  return parts[parts.length - 1] ?? key;
}

/** 路径尾段（最后两级），用作项目副标。 */
export function projectPathTail(key: string): string {
  const parts = key
    .replace(/[\\/]+$/, "")
    .split(/[\\/]/)
    .filter(Boolean);
  if (parts.length <= 1) return key;
  return `…/${parts.slice(-2).join("/")}`;
}

/**
 * 项目维度：普通项目按排行口径降序套紫色色阶；无明确项目／系统任务固定灰色排在其后；
 * 未归属（键为空串）固定最后。
 */
export function buildProjectRows(
  projects: UsageSummary | null,
  basis: RankingBasis,
): CompositionRow[] {
  const items = (projects?.rows ?? []).map((row) => ({
    key: row.key,
    name: projectNameFromKey(row.key),
    totals: rowTotals(row),
  }));
  const normal = byRankThenName(
    items.filter(
      (item) =>
        item.key !== PROJECT_KEY_UNATTRIBUTED &&
        item.key !== PROJECT_KEY_NONE &&
        item.key !== PROJECT_KEY_SYSTEM,
    ),
    basis,
  );
  const rows: CompositionRow[] = normal.map((item, index) => ({
    id: `project:${item.key}`,
    kind: "item",
    special: null,
    title: item.name,
    subtitle: projectPathTail(item.key),
    subtitleIsPath: true,
    color: { kind: "rank", index },
    totals: item.totals,
    action: { type: "project", key: item.key },
    sources: [],
    providerModels: [],
  }));
  for (const special of [PROJECT_KEY_NONE, PROJECT_KEY_SYSTEM] as const) {
    const item = items.find((candidate) => candidate.key === special);
    if (!item || !hasUsage(item.totals)) continue;
    rows.push({
      id: `project:${special}`,
      kind: "item",
      special: special === PROJECT_KEY_NONE ? "none" : "system",
      title: "",
      subtitle: "",
      subtitleIsPath: false,
      color: { kind: "rest" },
      totals: item.totals,
      action: { type: "project", key: special },
      sources: [],
      providerModels: [],
    });
  }
  const unattributed = items.find((item) => item.key === PROJECT_KEY_UNATTRIBUTED);
  if (unattributed && hasUsage(unattributed.totals)) {
    rows.push({
      id: "project:unattributed",
      kind: "unattributed",
      special: null,
      title: "",
      subtitle: "",
      subtitleIsPath: false,
      color: { kind: "unattributed" },
      totals: unattributed.totals,
      action: { type: "unattributed" },
      sources: [],
      providerModels: [],
    });
  }
  return rows;
}

// ---------- 周期归并 ----------

export interface PeriodSample {
  /** 周期起点 `YYYY-MM-DD`。 */
  key: string;
  start: Date;
  bySource: Partial<Record<UsageSource, UsageTotals>>;
  total: UsageTotals;
}

/**
 * 把各服务的日桶归并到所选粒度的周期（日／周／月）。底层桶始终是日桶，
 * 周／月只在读取时归并，与后端周键格式无关。
 */
export function buildPeriodSamples(
  day: Partial<Record<UsageSource, UsageSummary | null>>,
  sources: readonly UsageSource[],
  granularity: UsageGranularity,
): PeriodSample[] {
  const samples = new Map<string, PeriodSample>();
  for (const source of sources) {
    for (const row of day[source]?.rows ?? []) {
      const start = usageBucketStart(usageParseDayKey(row.key), granularity);
      const key = usageDayKey(start);
      const sample = samples.get(key) ?? { key, start, bySource: {}, total: emptyTotals() };
      const existing = sample.bySource[source] ?? emptyTotals();
      sample.bySource[source] = addTotals(existing, row);
      addTotals(sample.total, row);
      samples.set(key, sample);
    }
  }
  return [...samples.values()].sort((left, right) => left.start.getTime() - right.start.getTime());
}

/** 柱宽按样本数分档，不随面板宽度拉伸（cc-bar `barWidth`；30 根为 10）。 */
export function barWidthForCount(count: number): number {
  if (count <= 3) return 28;
  if (count <= 14) return 18;
  if (count <= 45) return 10;
  return 5;
}

// ---------- 高消耗对话 ----------

export function conversationTotals(conversation: UsageConversation): UsageTotals {
  return {
    entryCount: conversation.entryCount,
    requestCount: conversation.requestCount,
    tokens: conversation.tokens,
    fast: conversation.fast,
    cost: conversation.cost,
  };
}

/** 前 N 个对话合计占概览的比例（口径同排行）；合计为 0 或没有对话时返回 null。 */
export function topConversationsShare(
  conversations: readonly UsageConversation[],
  total: UsageTotals | null,
  basis: RankingBasis,
): number | null {
  if (!total || conversations.length === 0) return null;
  const denominator = rankValue(total, basis);
  if (denominator <= 0) return null;
  const sum = conversations.reduce(
    (accumulator, conversation) => accumulator + rankValue(conversationTotals(conversation), basis),
    0,
  );
  return Math.min(1, sum / denominator);
}

/** 排行口径为 Tokens 或费用时对话查询使用的排序。 */
export function conversationSortFor(basis: RankingBasis): "tokens" | "cost" {
  return basis === "cost" ? "cost" : "tokens";
}
