import { mount } from "@vue/test-utils";
import { createI18n } from "vue-i18n";
import { describe, expect, it } from "vitest";

import type { UsageProviderCosts } from "../features/usage/contracts";
import en from "../i18n/locales/en";
import zhCN from "../i18n/locales/zh-CN";
import UsageCostReadout from "./UsageCostReadout.vue";

function costs(overrides: Partial<UsageProviderCosts> = {}): UsageProviderCosts {
  return {
    today: {
      entryCount: 2,
      apiEquivalentCostNanos: 420_000_000,
      pricedEntries: 2,
      unpricedEntries: 0,
      assumedGeoEntries: 0,
    },
    week: {
      entryCount: 8,
      apiEquivalentCostNanos: 2_400_000_000,
      pricedEntries: 8,
      unpricedEntries: 0,
      assumedGeoEntries: 0,
    },
    ...overrides,
  };
}

function render(
  props: UsageProviderCosts,
  locale: "zh-CN" | "en" = "zh-CN",
  scanning = false,
) {
  const i18n = createI18n({
    legacy: false,
    locale,
    messages: { "zh-CN": zhCN, en },
  });

  return mount(UsageCostReadout, {
    props: {
      providerName: "Codex",
      costs: props,
      scanning,
    },
    global: { plugins: [i18n] },
  });
}

describe("UsageCostReadout", () => {
  it("keeps the initial load visibly unknown", () => {
    const wrapper = render(
      costs({
        today: null,
        week: null,
      }),
    );

    expect(wrapper.findAll("dd").map((amount) => amount.text())).toEqual(["—", "—"]);
    expect(wrapper.find(".usage-cost__caption").text()).toBe("花费");
  });

  it("renders compact today and week costs with accessible full amounts", () => {
    const wrapper = render(costs());
    const amounts = wrapper.findAll("dd");

    expect(wrapper.text()).toContain("今日");
    expect(wrapper.text()).toContain("本周");
    expect(amounts.map((amount) => amount.text())).toEqual(["<$1", "$2"]);
    expect(amounts[0].attributes("aria-label")).toContain("$0.42");
    expect(wrapper.attributes("aria-label")).toContain("Codex");
    expect(wrapper.find(".usage-cost__caption").text()).toBe("花费");
    expect(wrapper.find(".usage-cost__loading").exists()).toBe(false);
  });

  it("shows a small non-interactive loading status after the costs while scanning", () => {
    const wrapper = render(costs(), "zh-CN", true);

    expect(wrapper.find(".usage-cost__loading").exists()).toBe(true);
    expect(wrapper.find(".usage-cost__loading svg").exists()).toBe(true);
    expect(wrapper.find("button").exists()).toBe(false);
  });

  it("shows the priced subtotal without an amount suffix", () => {
    const wrapper = render(
      costs({
        today: {
          entryCount: 3,
          apiEquivalentCostNanos: 420_000_000,
          pricedEntries: 2,
          unpricedEntries: 1,
          assumedGeoEntries: 0,
        },
      }),
      "en",
    );

    expect(wrapper.findAll("dd").map((amount) => amount.text())).toEqual(["<$1", "$2"]);
    expect(wrapper.text()).not.toContain("+");
  });

  it("does not display a failed read as zero", () => {
    const wrapper = render(
      costs({
        today: null,
        week: null,
      }),
    );

    expect(wrapper.findAll("dd").map((amount) => amount.text())).toEqual(["—", "—"]);
    expect(wrapper.text()).not.toContain("$0");
  });
});
