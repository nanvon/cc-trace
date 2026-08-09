import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { createI18n } from "vue-i18n";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getUsageScanStatus, rebuildUsageData, refreshPricingCatalog } from "../features/usage/api";
import { useSettingsStore } from "../features/settings/store";
import en from "../i18n/locales/en";
import zhCN from "../i18n/locales/zh-CN";
import SettingsView from "./SettingsView.vue";

vi.mock("../features/usage/api", () => ({
  refreshPricingCatalog: vi.fn(),
  rebuildUsageData: vi.fn(),
  getUsageScanStatus: vi.fn(),
}));

function render() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const i18n = createI18n({
    legacy: false,
    locale: "zh-CN",
    messages: { "zh-CN": zhCN, en },
  });
  const wrapper = mount(SettingsView, {
    global: { plugins: [pinia, i18n] },
  });
  useSettingsStore(pinia).adopt({
    schemaVersion: 1,
    language: "zh-CN",
    appearance: "system",
    refreshInterval: "2m",
    launchAtLogin: false,
    privacyMode: false,
    showServiceStatus: true,
    onboarding: { completed: true, completedAt: null },
    usageServiceVisibility: { codex: true, claude: true, pi: true, opencode: true },
  });
  return wrapper;
}

describe("SettingsView pricing catalog", () => {
  beforeEach(() => {
    vi.mocked(refreshPricingCatalog).mockReset();
  });

  it("disables repeated updates while the request is pending and announces success", async () => {
    let resolveUpdate: ((value: "complete") => void) | undefined;
    vi.mocked(refreshPricingCatalog).mockReturnValue(
      new Promise<"complete">((resolve) => {
        resolveUpdate = resolve;
      }),
    );
    const wrapper = render();
    await wrapper.vm.$nextTick();
    const button = wrapper.get("button.flat-btn");

    await button.trigger("click");
    expect(button.attributes()).toHaveProperty("disabled");
    expect(button.text()).toBe("更新中…");

    resolveUpdate?.("complete");
    await flushPromises();
    expect(button.attributes()).not.toHaveProperty("disabled");
    expect(wrapper.text()).toContain("价格目录已是最新");
  });

  it("keeps the recovery message when both online sources fail", async () => {
    vi.mocked(refreshPricingCatalog).mockResolvedValue("failed");
    const wrapper = render();
    await wrapper.vm.$nextTick();

    await wrapper.get("button.flat-btn").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("当前仍使用原有目录");
    expect(wrapper.get("[aria-live='polite']").classes()).toContain(
      "settings__action-status--error",
    );
  });

  it("reports a partial update without pretending the whole catalog is current", async () => {
    vi.mocked(refreshPricingCatalog).mockResolvedValue("partial");
    const wrapper = render();
    await wrapper.vm.$nextTick();

    await wrapper.get("button.flat-btn").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("部分价格已更新");
    expect(wrapper.get("[aria-live='polite']").classes()).not.toContain(
      "settings__action-status--error",
    );
  });
});

describe("SettingsView data rebuild", () => {
  beforeEach(() => {
    vi.mocked(rebuildUsageData).mockReset();
    vi.mocked(getUsageScanStatus).mockReset();
  });

  const rebuildButton = (wrapper: ReturnType<typeof render>) => wrapper.get("[data-rebuild-btn]");

  it("asks for confirmation on the first click and resets after ten seconds", async () => {
    vi.useFakeTimers();
    const wrapper = render();
    await wrapper.vm.$nextTick();

    const button = rebuildButton(wrapper);
    expect(button.text()).toBe("重新计算用量");
    expect(rebuildUsageData).not.toHaveBeenCalled();

    await button.trigger("click");
    expect(button.text()).toBe("确认重新计算？");
    expect(rebuildUsageData).not.toHaveBeenCalled();

    vi.advanceTimersByTime(10_000);
    await flushPromises();
    expect(button.text()).toBe("重新计算用量");
    vi.useRealTimers();
  });

  it("rebuilds after confirmation and announces success when the scan finishes", async () => {
    vi.useFakeTimers();
    vi.mocked(rebuildUsageData).mockResolvedValue({
      state: "running",
      currentSource: null,
      discoveredFiles: 0,
      completedFiles: 0,
      bytesRead: 0,
      insertedEntries: 0,
      duplicateEntries: 0,
      invalidLines: 0,
      failedFiles: 0,
      partialFailure: false,
      cancelled: false,
      startedAt: "2026-08-09T00:00:00Z",
      finishedAt: null,
    });
    vi.mocked(getUsageScanStatus).mockResolvedValue({
      state: "idle",
      currentSource: null,
      discoveredFiles: 0,
      completedFiles: 0,
      bytesRead: 0,
      insertedEntries: 0,
      duplicateEntries: 0,
      invalidLines: 0,
      failedFiles: 0,
      partialFailure: false,
      cancelled: false,
      startedAt: "2026-08-09T00:00:00Z",
      finishedAt: "2026-08-09T00:00:02Z",
    });
    const wrapper = render();
    await wrapper.vm.$nextTick();

    await rebuildButton(wrapper).trigger("click");
    expect(rebuildUsageData).not.toHaveBeenCalled();

    await rebuildButton(wrapper).trigger("click");
    expect(rebuildUsageData).toHaveBeenCalledTimes(1);
    expect(rebuildButton(wrapper).text()).toBe("重新计算中…");
    expect(rebuildButton(wrapper).attributes()).toHaveProperty("disabled");

    await vi.advanceTimersByTimeAsync(1_000);
    expect(wrapper.text()).toContain("重新计算完成");
    expect(rebuildButton(wrapper).text()).toBe("重新计算用量");
    vi.useRealTimers();
  });

  it("reports failure when the rebuild request is rejected as busy", async () => {
    vi.mocked(rebuildUsageData).mockRejectedValue(new Error("busy"));
    const wrapper = render();
    await wrapper.vm.$nextTick();

    await rebuildButton(wrapper).trigger("click");
    await rebuildButton(wrapper).trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("重新计算失败");
    expect(wrapper.get("[aria-live='polite']").classes()).toContain(
      "settings__action-status--error",
    );
  });
});
