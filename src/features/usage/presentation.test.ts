import { describe, expect, it } from "vitest";

import type { UsageSummary } from "./contracts";
import { buildProviderCosts, presentUsageCost } from "./presentation";

const EMPTY_TOKENS = {
  uncachedInputTokens: 0,
  outputTokens: 0,
  reasoningOutputTokens: 0,
  cacheReadInputTokens: 0,
  cacheWrite5mInputTokens: 0,
  cacheWrite1hInputTokens: 0,
  inputTokens: 0,
  totalTokens: 0,
};

function summary(
  source: "codex" | "claude",
  options: { nanos: number; priced: number; unpriced: number },
): UsageSummary {
  const cost = {
    apiEquivalentCostNanos: options.nanos,
    pricedEntries: options.priced,
    unpricedEntries: options.unpriced,
    assumedGeoEntries: 0,
    pricingFingerprint: "fixture",
  };
  return {
    rows: [
      {
        key: source,
        entryCount: options.priced + options.unpriced,
        tokens: EMPTY_TOKENS,
        cost,
      },
    ],
    entryCount: options.priced + options.unpriced,
    tokens: EMPTY_TOKENS,
    cost,
  };
}

describe("buildProviderCosts", () => {
  it("turns a completed empty result into an honest zero", () => {
    const empty: UsageSummary = {
      rows: [],
      entryCount: 0,
      tokens: EMPTY_TOKENS,
      cost: {
        apiEquivalentCostNanos: 0,
        pricedEntries: 0,
        unpricedEntries: 0,
        assumedGeoEntries: 0,
        pricingFingerprint: null,
      },
    };

    const costs = buildProviderCosts("codex", empty, empty, true);
    expect(costs.today?.apiEquivalentCostNanos).toBe(0);
    expect(costs.week?.apiEquivalentCostNanos).toBe(0);
  });

  it("keeps a never-indexed empty result unknown", () => {
    const costs = buildProviderCosts("codex", null, null, false);

    expect(costs.today).toBeNull();
    expect(costs.week).toBeNull();
  });

  it("keeps indexed rows visible", () => {
    const indexed = summary("claude", { nanos: 2_000_000_000, priced: 2, unpriced: 0 });
    const costs = buildProviderCosts("claude", indexed, indexed, false);

    expect(costs.today?.apiEquivalentCostNanos).toBe(2_000_000_000);
  });
});

describe("presentUsageCost", () => {
  it("does not turn wholly unpriced usage into zero", () => {
    const indexed = summary("codex", { nanos: 0, priced: 0, unpriced: 3 });
    const costs = buildProviderCosts("codex", indexed, indexed, true);

    expect(presentUsageCost(costs.today)).toEqual({ amountNanos: null });
  });

  it("presents the priced subtotal without appending a lower-bound marker", () => {
    const indexed = summary("codex", { nanos: 900_000_000, priced: 2, unpriced: 1 });
    const costs = buildProviderCosts("codex", indexed, indexed, true);

    expect(presentUsageCost(costs.today)).toEqual({ amountNanos: 900_000_000 });
  });
});
