import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vitest/config";

/**
 * 前端测试只覆盖纯逻辑与状态映射；Tray、窗口与多显示器属人工实机验证，
 * 见 `docs/测试策略.md`。
 */
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
  },
});
