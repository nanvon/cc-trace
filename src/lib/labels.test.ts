import { describe, expect, it } from "vitest";

import type { QuotaWindow, QuotaWindowKind } from "../features/quota/contracts";
import { windowCode } from "./labels";

function makeWindow(kind: QuotaWindowKind, displayName: string | null = null): QuotaWindow {
  return {
    id: `${kind}-1`,
    kind,
    displayName,
    usedPercent: 20,
    remainingPercent: 80,
    resetsAt: null,
    windowSeconds: null,
    isActive: true,
    isPrimary: true,
  };
}

describe("windowCode", () => {
  it("uses the same short code for the shared five-hour window", () => {
    expect(windowCode("codex", makeWindow("fiveHour"))).toBe("5HOUR");
    expect(windowCode("claude", makeWindow("fiveHour"))).toBe("5HOUR");
  });

  it("reads Claude Code's weekly window as an all-model total", () => {
    expect(windowCode("claude", makeWindow("weekly"))).toBe("ALL");
    expect(windowCode("codex", makeWindow("weekly"))).toBe("WEEKLY");
  });

  it("upper-cases a model name so it matches the other codes", () => {
    expect(windowCode("claude", makeWindow("modelWeekly", "Opus"))).toBe("OPUS");
  });

  it("falls back to a code instead of rendering nothing", () => {
    expect(windowCode("claude", makeWindow("modelWeekly"))).toBe("MODEL");
    expect(windowCode("codex", makeWindow("unknown"))).toBe("CURRENT");
  });

  it("keeps every code short enough for the fixed label column", () => {
    const codes = [
      windowCode("codex", makeWindow("fiveHour")),
      windowCode("codex", makeWindow("weekly")),
      windowCode("claude", makeWindow("weekly")),
      windowCode("codex", makeWindow("unknown")),
    ];
    for (const code of codes) {
      expect(code).toMatch(/^[A-Z0-9]{3,7}$/);
    }
  });
});
