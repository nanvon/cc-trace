export const messages = {
  "zh-CN": {
    foundation: {
      title: "工程基础已经就位",
      description:
        "Tauri、Vue、Rust 与品牌资源已接入。正式额度界面将在信息架构和交互原型确认后开始。",
      statusTitle: "当前基线",
      stack: "Tauri 2 · Vue 3 · TypeScript · Rust",
      scope: "首版范围：Codex 与 Claude Code 额度监控",
      boundary: "不读取或迁移 Swift 版 cc-bar 应用数据",
    },
  },
  en: {
    foundation: {
      title: "The foundation is in place",
      description:
        "Tauri, Vue, Rust, and the brand assets are connected. The quota interface starts after the information architecture and interaction prototype are approved.",
      statusTitle: "Current baseline",
      stack: "Tauri 2 · Vue 3 · TypeScript · Rust",
      scope: "First release: Codex and Claude Code quota monitoring",
      boundary: "No Swift cc-bar application data is read or migrated",
    },
  },
} as const;
