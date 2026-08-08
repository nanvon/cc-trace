import { describe, expect, it } from "vitest";

import { isMainNavigationTarget, mainRoute } from "./navigation";

describe("main window navigation", () => {
  it("accepts only the documented targets", () => {
    expect(isMainNavigationTarget("quota")).toBe(true);
    expect(isMainNavigationTarget("settings")).toBe(true);
    expect(isMainNavigationTarget("timeline")).toBe(true);
    expect(isMainNavigationTarget("conversations")).toBe(true);
    expect(isMainNavigationTarget("compact")).toBe(false);
    expect(isMainNavigationTarget(null)).toBe(false);
  });

  it("maps external targets to routes without an origin query", () => {
    expect(mainRoute("quota")).toEqual({ name: "main" });
    expect(mainRoute("settings")).toEqual({ name: "settings" });
    expect(mainRoute("timeline")).toEqual({ name: "timeline" });
    expect(mainRoute("conversations")).toEqual({ name: "conversations" });
  });
});
