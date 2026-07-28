import { describe, expect, it } from "vitest";

import { displayQuotaTone, quotaTone } from "./quotaTone";

describe("quotaTone splits the four remaining-quota bands", () => {
  it("has no tone when there is no value at all", () => {
    expect(quotaTone(null)).toBe("none");
  });

  it("treats a fully consumed window as danger, not as a missing value", () => {
    expect(quotaTone(0)).toBe("danger");
  });

  it("keeps a sliver of quota out of the danger band", () => {
    // `<1%` 仍然是「还有额度」，不能和 0 同色，见 lib/format.ts 的 formatPercent
    expect(quotaTone(0.4)).toBe("low");
  });

  it("marks anything under 20% as low", () => {
    expect(quotaTone(19.9)).toBe("low");
  });

  it("puts the 20–50 range in warning, boundaries included", () => {
    expect(quotaTone(20)).toBe("warning");
    expect(quotaTone(50)).toBe("warning");
  });

  it("leaves more than half the window neutral", () => {
    expect(quotaTone(50.1)).toBe("ok");
    expect(quotaTone(100)).toBe("ok");
  });
});

describe("displayQuotaTone downgrades anything that is not a current value", () => {
  it("colors current values by their band", () => {
    expect(displayQuotaTone(12, "filled")).toBe("low");
  });

  it("refuses to claim a stale snapshot is tight", () => {
    expect(displayQuotaTone(12, "faded")).toBe("none");
  });

  it("has no tone while loading or when there is nothing to show", () => {
    expect(displayQuotaTone(null, "loading")).toBe("none");
    expect(displayQuotaTone(null, "empty")).toBe("none");
  });
});
