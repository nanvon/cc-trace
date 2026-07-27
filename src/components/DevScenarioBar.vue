<script setup lang="ts">
/**
 * 验证场景切换器。**只存在于 debug 构建**：release 里 `import.meta.env.DEV` 为 false，
 * 对应的 `dev_set_scenario` 命令也被 `#[cfg(debug_assertions)]` 编译掉。
 *
 * 它切换的是合成 Provider 的内部场景，数据仍走 `quota://updated` 这一条路径，
 * 不是第二套状态源。默认停在「真实数据」，切到任一合成场景才会脱离真实 Provider。
 */
import { invoke } from "@tauri-apps/api/core";
import { ref } from "vue";
import { useI18n } from "vue-i18n";

const SCENARIOS = [
  "live",
  "healthy",
  "firstLoad",
  "noCredentials",
  "unsupported",
  "offlineStale",
  "offlineEmpty",
  "rateLimited",
  "errorStale",
  "errorEmpty",
] as const;

const { t } = useI18n();
// 默认与 Rust 侧一致：debug 构建也走真实 Provider，合成场景只有显式切换才生效。
const scenario = ref<(typeof SCENARIOS)[number]>("live");

async function apply(): Promise<void> {
  try {
    await invoke("dev_set_scenario", { scenario: scenario.value });
  } catch {
    // 纯浏览器预览没有命令桥，忽略即可。
  }
}
</script>

<template>
  <aside class="dev">
    <label class="dev__field">
      <span class="utility-label">{{ t("preview.scenario") }}</span>
      <select v-model="scenario" autocomplete="off" @change="apply">
        <option v-for="option in SCENARIOS" :key="option" :value="option">
          {{ t(`preview.scenarioOption.${option}`) }}
        </option>
      </select>
    </label>
  </aside>
</template>

<style scoped>
.dev {
  padding: var(--space-3) var(--space-4);
  border: 1px dashed var(--border-subtle);
  border-radius: var(--radius-small);
}

.dev__field {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

select {
  min-height: 2rem;
  padding: 0 var(--space-2);
  color: var(--text-primary);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
}
</style>
