import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vitest/config";

/**
 * 前端测试覆盖纯逻辑、状态映射与不依赖真实窗口的组件渲染；Tray、窗口与多显示器属
 * 人工实机验证，见 `docs/测试策略.md`。
 */
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
  },
});
