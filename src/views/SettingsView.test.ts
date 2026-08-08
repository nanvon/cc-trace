import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { createI18n } from "vue-i18n";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { refreshPricingCatalog } from "../features/usage/api";
import { useSettingsStore } from "../features/settings/store";
import en from "../i18n/locales/en";
import zhCN from "../i18n/locales/zh-CN";
import SettingsView from "./SettingsView.vue";

vi.mock("vue-router", () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({}),
}));

vi.mock("../features/app/navigation", () => ({
  navigateMain: vi.fn(),
}));

vi.mock("../features/usage/api", () => ({
  refreshPricingCatalog: vi.fn(),
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
    const button = wrapper.get("button.button--flat");

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

    await wrapper.get("button.button--flat").trigger("click");
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

    await wrapper.get("button.button--flat").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("部分价格已更新");
    expect(wrapper.get("[aria-live='polite']").classes()).not.toContain(
      "settings__action-status--error",
    );
  });
});
