import js from "@eslint/js";
import vue from "eslint-plugin-vue";
import typescript from "typescript-eslint";
import prettier from "eslint-config-prettier";

/**
 * Lint 只保证正确性与一致性；格式交给 Prettier，两者不重叠。
 * 范围与命令见 `docs/工程与发布.md` 第 2、3 节。
 */
export default typescript.config(
  {
    ignores: ["dist/**", "src-tauri/**", "prototypes/**", "node_modules/**"],
  },
  js.configs.recommended,
  ...typescript.configs.recommended,
  ...vue.configs["flat/recommended"],
  {
    files: ["**/*.vue"],
    languageOptions: {
      parserOptions: {
        parser: typescript.parser,
      },
    },
  },
  {
    rules: {
      // TypeScript 自己就检查未定义标识符，且比 ESLint 更准确地知道 DOM 全局；
      // 保留这条规则只会对 window / document / HTMLElement 误报。
      "no-undef": "off",
      // 组件文件名已经表达了层级，单词组件名在 views/ 下是有意为之
      "vue/multi-word-component-names": "off",
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
  prettier,
);
