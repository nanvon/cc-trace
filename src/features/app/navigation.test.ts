import { describe, expect, it } from "vitest";

import { isMainNavigationTarget, mainRoute } from "./navigation";

describe("main window navigation", () => {
  it("accepts only the two documented targets", () => {
    expect(isMainNavigationTarget("quota")).toBe(true);
    expect(isMainNavigationTarget("settings")).toBe(true);
    expect(isMainNavigationTarget("compact")).toBe(false);
    expect(isMainNavigationTarget(null)).toBe(false);
  });

  it("maps external targets without persisting an origin", () => {
    expect(mainRoute("quota")).toEqual({ name: "main" });
    expect(mainRoute("settings")).toEqual({
      name: "settings",
      query: undefined,
    });
  });

  it("keeps the quota origin only long enough to restore focus", () => {
    expect(mainRoute("settings", "quota")).toEqual({
      name: "settings",
      query: { origin: "quota" },
    });
  });
});
