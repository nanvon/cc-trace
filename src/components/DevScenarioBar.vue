<script setup lang="ts">
/**
 * 验证场景切换器。**只存在于 debug 构建**：release 里 `import.meta.env.DEV` 为 false，
 * 对应的 `dev_set_scenario` 命令也被 `#[cfg(debug_assertions)]` 编译掉。
 *
 * 它切换的是合成 Provider 的内部场景，数据仍走 `quota://updated` 这一条路径，
 * 不是第二套状态源。默认停在「真实数据」，切到任一合成场景才会脱离真实 Provider。
 */
import { invoke } from "@tauri-apps/api/core";
import { computed, ref } from "vue";
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

/**
 * 「这些数字不是真的」这句话只在确实脱离真实 Provider 时才成立。
 * 它必须跟着场景走：挂在真实额度下面会直接摧毁数据可信度。
 */
const isSynthetic = computed(() => scenario.value !== "live");

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

    <p v-if="isSynthetic" class="dev__notice supporting" role="status">
      <span class="utility-label">{{ t("preview.badge") }}</span>
      {{ t("preview.notice") }}
    </p>
  </aside>
</template>

<style scoped>
.dev {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border: 1px dashed var(--border-subtle);
  border-radius: var(--radius-small);
}

.dev__field {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.dev__notice {
  margin: 0;
  max-inline-size: 42rem;
  font-size: 0.8125rem;
  line-height: 1.6;
}

.dev__notice .utility-label {
  margin-inline-end: var(--space-2);
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
