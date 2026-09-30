import { describe, expect, it } from "vitest";

import type { UsageConversation, UsageSource, UsageSummary, UsageSummaryRow } from "./contracts";
import {
  barWidthForCount,
  buildModelRows,
  buildPeriodSamples,
  buildProjectRows,
  buildProviderRows,
  buildServiceRows,
  emptyTotals,
  rankRatio,
  rankShare,
  resolveModelProvider,
  topConversationsShare,
} from "./overview";

function row(key: string, tokens: number, costNanos: number, fastTokens = 0): UsageSummaryRow {
  const totals = emptyTotals();
  totals.tokens.totalTokens = tokens;
  totals.tokens.inputTokens = tokens;
  totals.cost.apiEquivalentCostNanos = costNanos;
  totals.cost.pricedEntries = 1;
  totals.entryCount = 1;
  totals.fast.rawTokens = fastTokens;
  return { key, ...totals };
}

function summary(rows: UsageSummaryRow[]): UsageSummary {
  const total = emptyTotals();
  for (const item of rows) {
    total.entryCount += item.entryCount;
    total.tokens.totalTokens += item.tokens.totalTokens;
    total.cost.apiEquivalentCostNanos += item.cost.apiEquivalentCostNanos;
  }
  return { rows, ...total };
}

function shareSum(
  rows: ReturnType<typeof buildServiceRows>,
  total: ReturnType<typeof emptyTotals>,
  basis: "tokens" | "cost",
) {
  return rows.reduce((sum, item) => sum + rankShare(item.totals, total, basis), 0);
}

describe("resolveModelProvider", () => {
  it("resolves prefix, keyword and source fallback in that order", () => {
    expect(resolveModelProvider("pi", "openai-codex/gpt-5")).toBe("openai");
    expect(resolveModelProvider("opencode", "opencode-go/kimi")).toBe("opencodeGo");
    expect(resolveModelProvider("dsh", "deepseek-v4")).toBe("deepseek");
    expect(resolveModelProvider("cursor", "claude-sonnet-4")).toBe("anthropic");
    expect(resolveModelProvider("codex", "unknown-model")).toBe("openai");
    expect(resolveModelProvider("claude", "unknown-model")).toBe("anthropic");
    expect(resolveModelProvider("pi", "unknown-model")).toBe("other");
  });
});

describe("composition rows", () => {
  const model: Partial<Record<UsageSource, UsageSummary>> = {
    codex: summary([row("gpt-5", 600, 6_000), row("gpt-5-mini", 100, 1_000)]),
    claude: summary([row("claude-sonnet-4", 300, 9_000)]),
    pi: summary([row("mystery/model", 50, 500)]),
  };
  const sources: UsageSource[] = ["codex", "claude", "pi"];
  const source = summary([row("codex", 700, 7_000), row("claude", 300, 9_000), row("pi", 50, 500)]);
  const total = { ...emptyTotals(), ...source };

  it("ranks services by the ranking basis and keeps shares summing to 100%", () => {
    const byCost = buildServiceRows(source, sources, "cost");
    expect(byCost.map((item) => item.sources[0])).toEqual(["claude", "codex", "pi"]);
    expect(shareSum(byCost, total, "cost")).toBeCloseTo(1, 10);

    const byTokens = buildServiceRows(source, sources, "tokens");
    expect(byTokens.map((item) => item.sources[0])).toEqual(["codex", "claude", "pi"]);
    expect(shareSum(byTokens, total, "tokens")).toBeCloseTo(1, 10);
  });

  it("groups models by derived provider with the other group last", () => {
    const rows = buildProviderRows(model, sources, "cost");
    expect(rows.map((item) => item.title)).toEqual(["anthropic", "openai", "other"]);
    expect(rows[2]?.color).toEqual({ kind: "rest" });
    expect(rows[1]?.providerModels.map((item) => item.model)).toEqual(["gpt-5", "gpt-5-mini"]);
    expect(shareSum(rows, total, "cost")).toBeCloseTo(1, 10);
  });

  it("merges the same model across sources and assigns purple rank colors", () => {
    const merged = buildModelRows(
      { codex: summary([row("gpt-5", 100, 100)]), pi: summary([row("gpt-5", 50, 50)]) },
      ["codex", "pi"],
      "tokens",
    );
    expect(merged).toHaveLength(1);
    expect(merged[0]?.totals.tokens.totalTokens).toBe(150);
    expect(merged[0]?.sources).toEqual(["codex", "pi"]);
    expect(merged[0]?.color).toEqual({ kind: "rank", index: 0 });
  });

  it("falls back to tokens for the share when nothing is priced", () => {
    const zeroCost = { ...emptyTotals(), ...summary([row("a", 100, 0), row("b", 300, 0)]) };
    expect(rankShare(row("a", 100, 0), zeroCost, "cost")).toBeCloseTo(0.25, 10);
  });
});

describe("project rows", () => {
  const projects = summary([
    row("D:\\code\\small", 100, 1_000),
    row("/home/a/big", 900, 9_000),
    row("@none", 50, 400),
    row("@system", 30, 200),
    row("", 70, 300),
  ]);

  it("ranks projects then special rows then unattributed, and reconciles to the total", () => {
    const rows = buildProjectRows(projects, "cost");
    expect(rows.map((item) => item.id)).toEqual([
      "project:/home/a/big",
      "project:D:\\code\\small",
      "project:@none",
      "project:@system",
      "project:unattributed",
    ]);
    expect(rows[0]?.title).toBe("big");
    expect(rows[1]?.title).toBe("small");
    expect(rows[2]?.special).toBe("none");
    expect(rows[4]?.kind).toBe("unattributed");
    expect(rows[4]?.color).toEqual({ kind: "unattributed" });
    const tokens = rows.reduce((sum, item) => sum + item.totals.tokens.totalTokens, 0);
    expect(tokens).toBe(1150);
    const total = { ...emptyTotals(), ...projects };
    expect(shareSum(rows, total, "tokens")).toBeCloseTo(1, 10);
  });

  it("omits unattributed and special rows that have no usage", () => {
    const rows = buildProjectRows(
      summary([row("/a/b", 10, 10), { ...row("", 0, 0), entryCount: 0 }]),
      "cost",
    );
    expect(rows.map((item) => item.id)).toEqual(["project:/a/b"]);
  });
});

describe("period samples", () => {
  const day = {
    codex: summary([row("2026-07-27", 10, 1), row("2026-07-29", 20, 2), row("2026-08-02", 5, 1)]),
    claude: summary([row("2026-07-28", 7, 1)]),
  };

  it("keeps daily buckets for day granularity", () => {
    const samples = buildPeriodSamples(day, ["codex", "claude"], "day");
    expect(samples.map((item) => item.key)).toEqual([
      "2026-07-27",
      "2026-07-28",
      "2026-07-29",
      "2026-08-02",
    ]);
  });

  it("folds days into Monday weeks and calendar months without losing tokens", () => {
    const weeks = buildPeriodSamples(day, ["codex", "claude"], "week");
    expect(weeks.map((item) => item.key)).toEqual(["2026-07-27"]);
    expect(weeks[0]?.total.tokens.totalTokens).toBe(42);
    expect(weeks[0]?.bySource.codex?.tokens.totalTokens).toBe(35);

    const months = buildPeriodSamples(day, ["codex", "claude"], "month");
    expect(months.map((item) => item.key)).toEqual(["2026-07-01", "2026-08-01"]);
    expect(months[0]?.total.tokens.totalTokens).toBe(37);
  });

  it("uses the agreed bar width tiers", () => {
    expect(barWidthForCount(2)).toBe(28);
    expect(barWidthForCount(10)).toBe(18);
    expect(barWidthForCount(30)).toBe(10);
    expect(barWidthForCount(60)).toBe(5);
  });
});

describe("top conversations", () => {
  function conversation(key: string, tokens: number, cost: number): UsageConversation {
    const totals = emptyTotals();
    totals.tokens.totalTokens = tokens;
    totals.cost.apiEquivalentCostNanos = cost;
    return {
      conversationKey: key,
      source: "codex",
      title: key,
      projectHint: null,
      projectKey: null,
      worktreePath: null,
      unattributed: false,
      isSidechain: false,
      firstAt: "2026-07-01T00:00:00Z",
      lastAt: "2026-07-02T00:00:00Z",
      entryCount: 1,
      requestCount: 1,
      tokens: totals.tokens,
      fast: totals.fast,
      cost: totals.cost,
      sourceId: null,
      branch: null,
      models: [],
    };
  }

  it("computes the top-N share against the overview total per ranking basis", () => {
    const total = { ...emptyTotals(), ...summary([row("x", 1_000, 10_000)]) };
    const list = [conversation("a", 400, 4_000), conversation("b", 100, 1_000)];
    expect(topConversationsShare(list, total, "tokens")).toBeCloseTo(0.5, 10);
    expect(topConversationsShare(list, total, "cost")).toBeCloseTo(0.5, 10);
    expect(topConversationsShare([], total, "cost")).toBeNull();
    expect(topConversationsShare(list, emptyTotals(), "cost")).toBeNull();
  });

  it("normalizes bar length against the first place and clamps", () => {
    expect(rankRatio(50, 100)).toBe(0.5);
    expect(rankRatio(150, 100)).toBe(1);
    expect(rankRatio(5, 0)).toBe(0);
  });
});
