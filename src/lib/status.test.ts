import { describe, expect, it } from "vitest";

import type { ProviderSnapshot } from "../features/quota/contracts";
import { hasQuotaValues, presentOverall, presentProvider } from "./status";

function snapshot(overrides: Partial<ProviderSnapshot> = {}): ProviderSnapshot {
  return {
    provider: "codex",
    refresh: "idle",
    freshness: "live",
    availability: "ready",
    identity: null,
    snapshot: {
      windows: [
        {
          id: "codex.primary",
          kind: "fiveHour",
          displayName: null,
          usedPercent: 27,
          remainingPercent: 73,
          resetsAt: "2026-07-25T14:30:00Z",
          windowSeconds: 18000,
          isActive: true,
          isPrimary: true,
        },
      ],
      capturedAt: "2026-07-25T10:00:00Z",
    },
    lastSuccessAt: "2026-07-25T10:00:00Z",
    lastAttemptAt: "2026-07-25T10:00:00Z",
    retryAfter: null,
    error: null,
    ...overrides,
  };
}

describe("presentProvider covers the status matrix", () => {
  it("first load shows a skeleton rail, not an empty quota", () => {
    const result = presentProvider(
      snapshot({ refresh: "loading", freshness: "empty", snapshot: null, lastSuccessAt: null }),
    );

    expect(result.titleKey).toBe("status.loading");
    expect(result.rail).toBe("loading");
  });

  it("refreshing with a snapshot keeps the filled rail", () => {
    const result = presentProvider(snapshot({ refresh: "refreshing" }));

    expect(result.titleKey).toBe("status.refreshing");
    expect(result.rail).toBe("filled");
  });

  it("live is neutral, never a celebratory green banner", () => {
    const result = presentProvider(snapshot());

    expect(result.titleKey).toBe("status.live");
    expect(result.tone).toBe("neutral");
    expect(result.nextStepKey).toBeNull();
  });

  it("no_credentials is neutral and shows no rail at all", () => {
    const result = presentProvider(
      snapshot({ availability: "no_credentials", freshness: "empty", snapshot: null }),
    );

    expect(result.titleKey).toBe("status.noCredentials");
    expect(result.tone).toBe("neutral");
    expect(result.rail).toBe("empty");
    expect(result.nextStepKey).toBe("nextStep.noCredentials");
  });

  it("unsupported is not downgraded to a generic error", () => {
    const result = presentProvider(
      snapshot({ availability: "unsupported", freshness: "empty", snapshot: null }),
    );

    expect(result.titleKey).toBe("status.unsupported");
    expect(result.tone).not.toBe("critical");
  });

  it("offline with a snapshot fades the rail but keeps the values", () => {
    const result = presentProvider(snapshot({ availability: "offline", freshness: "stale" }));

    expect(result.titleKey).toBe("status.offlineStale");
    expect(result.rail).toBe("faded");
    expect(result.tone).toBe("warning");
  });

  it("offline without a snapshot is never a grey dot", () => {
    const result = presentProvider(
      snapshot({ availability: "offline", freshness: "empty", snapshot: null }),
    );

    expect(result.titleKey).toBe("status.offlineEmpty");
    expect(result.tone).toBe("warning");
    expect(result.nextStepKey).toBe("nextStep.offlineEmpty");
  });

  it("rate_limited keeps the old values and offers a retry time", () => {
    const result = presentProvider(
      snapshot({
        availability: "rate_limited",
        freshness: "stale",
        retryAfter: "2026-07-25T10:12:00Z",
      }),
    );

    expect(result.titleKey).toBe("status.rateLimited");
    expect(result.rail).toBe("faded");
    expect(result.nextStepKey).toBe("nextStep.rateLimited");
  });

  it("distinguishes credential errors from protocol errors", () => {
    const credentials = presentProvider(
      snapshot({ availability: "error", freshness: "stale", error: { kind: "credentials" } }),
    );
    const protocol = presentProvider(
      snapshot({
        availability: "error",
        freshness: "empty",
        snapshot: null,
        error: { kind: "protocol" },
      }),
    );

    expect(credentials.titleKey).toBe("status.errorCredentials");
    expect(credentials.nextStepKey).toBe("nextStep.errorCredentials");
    expect(protocol.titleKey).toBe("status.errorProtocol");
    expect(protocol.nextStepKey).toBe("nextStep.errorProtocol");
  });

  it("an aged-out snapshot goes stale without inventing a failure reason", () => {
    const result = presentProvider(snapshot({ freshness: "stale" }));

    expect(result.titleKey).toBe("status.stale");
    expect(result.rail).toBe("faded");
    expect(result.nextStepKey).toBe("nextStep.stale");
  });
});

describe("presentOverall", () => {
  it("surfaces the highest risk without reordering providers", () => {
    const healthy = snapshot({ provider: "claude" });
    const failing = snapshot({
      provider: "codex",
      availability: "error",
      freshness: "empty",
      snapshot: null,
      error: { kind: "credentials" },
    });

    const leader = presentOverall([failing, healthy]);

    expect(leader?.provider.provider).toBe("codex");
    expect(leader?.presentation.titleKey).toBe("status.errorCredentials");
  });

  it("keeps the first provider when risk is equal, so nothing jumps on refresh", () => {
    const leader = presentOverall([
      snapshot({ provider: "codex" }),
      snapshot({ provider: "claude" }),
    ]);

    expect(leader?.provider.provider).toBe("codex");
  });

  it("returns null when there is nothing to show yet", () => {
    expect(presentOverall([])).toBeNull();
  });
});

describe("hasQuotaValues", () => {
  it("is false whenever the freshness says empty", () => {
    expect(hasQuotaValues(snapshot())).toBe(true);
    expect(hasQuotaValues(snapshot({ freshness: "empty", snapshot: null }))).toBe(false);
  });
});
