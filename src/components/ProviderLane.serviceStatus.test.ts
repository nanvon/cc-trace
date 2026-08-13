import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { createI18n } from "vue-i18n";
import { describe, expect, it } from "vitest";

import type { ProviderSnapshot } from "../features/quota/contracts";
import type { ServiceStatus } from "../features/quota/serviceStatus";
import { useSettingsStore } from "../features/settings/store";
import en from "../i18n/locales/en";
import zhCN from "../i18n/locales/zh-CN";
import ProviderLane from "./ProviderLane.vue";

function snapshot(): ProviderSnapshot {
  return {
    provider: "codex",
    refresh: "idle",
    freshness: "empty",
    availability: "ready",
    identity: null,
    snapshot: null,
    lastSuccessAt: null,
    lastAttemptAt: null,
    retryAfter: null,
    error: null,
  };
}

function status(indicator: ServiceStatus["indicator"]): ServiceStatus {
  return {
    indicator,
    description: null,
    updatedAt: null,
    fetchedAt: "2026-08-09T00:00:00Z",
  };
}

function render(serviceStatus?: ServiceStatus | null, showServiceStatus = true) {
  const pinia = createPinia();
  setActivePinia(pinia);
  useSettingsStore(pinia).adopt({
    schemaVersion: 1,
    language: "zh-CN",
    appearance: "system",
    refreshInterval: "2m",
    launchAtLogin: false,
    privacyMode: false,
    showServiceStatus,
    onboarding: { completed: true, completedAt: null },
    usageServiceVisibility: { codex: true, claude: true, pi: true, opencode: true },
  });
  const i18n = createI18n({
    legacy: false,
    locale: "zh-CN",
    messages: { "zh-CN": zhCN, en },
  });
  return mount(ProviderLane, {
    props: {
      provider: snapshot(),
      variant: "compact",
      serviceStatus: serviceStatus ?? undefined,
    },
    global: { plugins: [pinia, i18n] },
  });
}

describe("ProviderLane service status dot (ADR-0026)", () => {
  it("draws a green dot for a healthy service", async () => {
    const wrapper = render(status("none"));
    await flushPromises();
    const dot = wrapper.get(".lane-status");
    expect(dot.classes()).toContain("lane-status--success");
    expect(dot.attributes("aria-label")).toBe("服务正常");
  });

  it("maps each indicator to its own tone", async () => {
    const expected: Record<ServiceStatus["indicator"], string> = {
      none: "lane-status--success",
      minor: "lane-status--warning",
      major: "lane-status--low",
      critical: "lane-status--error",
      maintenance: "lane-status--maintenance",
      unknown: "",
    };
    for (const [indicator, tone] of Object.entries(expected)) {
      const wrapper = render(status(indicator as ServiceStatus["indicator"]));
      await flushPromises();
      const dot = wrapper.find(".lane-status");
      if (tone === "") {
        expect(dot.exists()).toBe(false);
      } else {
        expect(dot.classes()).toContain(tone);
      }
      wrapper.unmount();
    }
  });

  it("does not draw a dot before the first fetch or when unknown", async () => {
    const withoutData = render(null);
    await flushPromises();
    expect(withoutData.find(".lane-status").exists()).toBe(false);

    const unknown = render(status("unknown"));
    await flushPromises();
    expect(unknown.find(".lane-status").exists()).toBe(false);
  });

  it("hides the dot when the setting is off, while the data still arrives", async () => {
    const wrapper = render(status("critical"), false);
    await flushPromises();
    expect(wrapper.find(".lane-status").exists()).toBe(false);
  });

  it("prefers the description and appends the page update age to the tooltip", async () => {
    const fetchedAt = new Date();
    const wrapper = render({
      indicator: "major",
      description: "We are investigating elevated errors",
      updatedAt: new Date(fetchedAt.getTime() - 5 * 60_000).toISOString(),
      fetchedAt: fetchedAt.toISOString(),
    });
    await flushPromises();
    const dot = wrapper.get(".lane-status");
    expect(dot.attributes("title")).toContain("We are investigating elevated errors");
    expect(dot.attributes("title")).toContain("前更新");
    // 时间词已含「前」后缀（formatPast 的 Intl 结果），模板不得再拼一次后缀。
    expect(dot.attributes("title")).not.toContain("前 前更新");
    expect(dot.attributes("aria-label")).toBe("重大故障");
  });

  it("falls back to the indicator label when the description is missing", async () => {
    const wrapper = render(status("minor"));
    await flushPromises();
    const dot = wrapper.get(".lane-status");
    expect(dot.attributes("title")).toBe("轻微故障");
  });
});
