<script setup lang="ts">
/**
 * 主窗口设置视图：四个分类（cc-bar 同款）——服务与账号、外观与显示、数据与刷新、通用。
 *
 * 保存成功立即生效；写入失败时**保留原值**并明确提示。
 * 账号与凭据只经 Rust 命令搬运，本视图不接触 token 原文（提交后立即清空输入）。
 * 隐私模式下账号邮箱经 `usePrivacy()` 遮挡，只改展示，不改数据。
 * 导航由侧边栏承担（ADR-0024）：本视图不再提供「返回用量」按钮。
 */
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import { usePrivacy } from "../lib/privacy";
import { presentProvider } from "../lib/status";
import { useQuotaStore } from "../features/quota/store";
import { getUsageScanStatus, rebuildUsageData, refreshPricingCatalog } from "../features/usage/api";
import {
  clearCommandCodeApiKey,
  commandErrorCode,
  getCodexAccounts,
  getCommandCodeCredentialState,
  importCodexAccount,
  removeCodexAccount,
  reorderCodexAccounts,
  setCommandCodeApiKey,
  updateCodexAccount,
  type ImportedCodexAccount,
} from "../features/settings/accounts";
import {
  APPEARANCE_OPTIONS,
  LANGUAGE_OPTIONS,
  MENU_BAR_WINDOW_MODE_OPTIONS,
  RANKING_BASIS_OPTIONS,
  REFRESH_INTERVAL_OPTIONS,
  RESET_TIME_DISPLAY_OPTIONS,
  type AppearancePreference,
  type CommandCodeCredentialState,
  type LanguagePreference,
  type MenuBarWindowMode,
  type RankingBasis,
  type RefreshIntervalOption,
  type ResetTimeDisplay,
  type ServiceSettings,
  type ServicesSettings,
  type SettingsUpdate,
} from "../features/settings/contracts";
import {
  checkForUpdates,
  exportDiagnostics,
  getCredentialSources,
  getUpdateStatus,
  onUpdateStatus,
  openReleasePage,
  revealLogFolder,
  type CredentialSources,
  type UpdateStatus,
} from "../features/settings/maintenance";
import { useSettingsStore } from "../features/settings/store";

type Category = "services" | "appearance" | "data" | "general";
const CATEGORIES: readonly Category[] = ["services", "appearance", "data", "general"] as const;

type QuotaService = "codex" | "claude" | "antigravity" | "cursor" | "commandCode";
const QUOTA_SERVICES: readonly QuotaService[] = [
  "codex",
  "claude",
  "antigravity",
  "cursor",
  "commandCode",
] as const;
const SERVICE_FIELDS: readonly (keyof ServiceSettings)[] = [
  "quota",
  "menuBar",
  "hud",
  "stats",
] as const;

const { t } = useI18n();
const settings = useSettingsStore();
const privacy = usePrivacy();
const quota = useQuotaStore();
const category = ref<Category>("services");
const current = computed(() => settings.settings);

// ---- 价格目录与数据重建 ----
const pricingRefreshState = ref<"idle" | "success" | "partial" | "failure">("idle");
const pricingRefreshPending = ref(false);
const PRICING_REFRESH_STATE = {
  complete: "success",
  partial: "partial",
  failed: "failure",
} as const;

/** 数据重建：idle → confirm（二次确认防误触）→ running → success/failure。 */
const rebuildState = ref<"idle" | "confirm" | "running" | "success" | "failure">("idle");
const rebuildPending = ref(false);
let rebuildConfirmTimer: ReturnType<typeof setTimeout> | null = null;
let rebuildPoll: ReturnType<typeof setInterval> | null = null;

const rebuildLabel = computed(() => {
  if (rebuildState.value === "confirm") return t("settings.rebuildConfirm");
  if (rebuildPending.value) return t("settings.rebuildRunning");
  return t("settings.rebuildUsage");
});

const rebuildStatusText = computed(() => {
  switch (rebuildState.value) {
    case "running":
      return t("settings.rebuildRunningHint");
    case "success":
      return t("settings.rebuildSuccess");
    case "failure":
      return t("settings.rebuildFailure");
    default:
      return "";
  }
});

function scheduleRebuildConfirmReset(): void {
  if (rebuildConfirmTimer) clearTimeout(rebuildConfirmTimer);
  rebuildConfirmTimer = setTimeout(() => {
    if (rebuildState.value === "confirm") rebuildState.value = "idle";
  }, 10_000);
}

function clearRebuildPoll(): void {
  if (rebuildPoll) {
    clearInterval(rebuildPoll);
    rebuildPoll = null;
  }
}

/**
 * 首次点击进入确认态（10 秒未确认自动退回）；确认后删除本地统计并全量重扫，
 * 期间轮询扫描状态，结束后给出完成反馈。扫描中触发返回 busy，直接报失败文案。
 */
async function requestRebuild(): Promise<void> {
  if (rebuildPending.value) return;
  if (rebuildState.value !== "confirm") {
    rebuildState.value = "confirm";
    scheduleRebuildConfirmReset();
    return;
  }
  if (rebuildConfirmTimer) {
    clearTimeout(rebuildConfirmTimer);
    rebuildConfirmTimer = null;
  }
  rebuildPending.value = true;
  rebuildState.value = "running";
  try {
    await rebuildUsageData();
    rebuildPoll = setInterval(async () => {
      try {
        const status = await getUsageScanStatus();
        if (status.state === "running" || status.state === "cancelling") return;
        clearRebuildPoll();
        rebuildState.value = "success";
        rebuildPending.value = false;
      } catch {
        clearRebuildPoll();
        rebuildState.value = "failure";
        rebuildPending.value = false;
      }
    }, 1_000);
  } catch {
    rebuildState.value = "failure";
    rebuildPending.value = false;
  }
}

async function updatePricingCatalog(): Promise<void> {
  if (pricingRefreshPending.value) return;
  pricingRefreshPending.value = true;
  pricingRefreshState.value = "idle";
  try {
    const result = await refreshPricingCatalog();
    pricingRefreshState.value = PRICING_REFRESH_STATE[result];
  } catch {
    pricingRefreshState.value = "failure";
  } finally {
    pricingRefreshPending.value = false;
  }
}

// ---- 设置项写入 ----
const INTERVAL_LABEL: Record<RefreshIntervalOption, string> = {
  "1m": "settings.intervalOption.m1",
  "2m": "settings.intervalOption.m2",
  "3m": "settings.intervalOption.m3",
  "5m": "settings.intervalOption.m5",
  "10m": "settings.intervalOption.m10",
};

const LANGUAGE_LABEL: Record<LanguagePreference, string> = {
  system: "settings.languageOption.system",
  "zh-CN": "settings.languageOption.chinese",
  en: "settings.languageOption.english",
};

const APPEARANCE_LABEL: Record<AppearancePreference, string> = {
  system: "settings.appearanceOption.system",
  light: "settings.appearanceOption.light",
  dark: "settings.appearanceOption.dark",
};

const MENU_BAR_MODE_LABEL: Record<MenuBarWindowMode, string> = {
  primary: "settings.menuBarModeOption.primary",
  weekly: "settings.menuBarModeOption.weekly",
  both: "settings.menuBarModeOption.both",
};

const RANKING_LABEL: Record<RankingBasis, string> = {
  tokens: "settings.rankingOption.tokens",
  cost: "settings.rankingOption.cost",
};

const RESET_TIME_LABEL: Record<ResetTimeDisplay, string> = {
  duration: "settings.resetTimeOption.duration",
  dateTime: "settings.resetTimeOption.dateTime",
};

/** 写入失败时 store 保持原值，控件由 :value／:checked 绑定自然回到原值。 */
async function commitSelect(
  key:
    | "refreshInterval"
    | "scanInterval"
    | "language"
    | "appearance"
    | "menuBarWindowMode"
    | "rankingBasis"
    | "resetTimeDisplay",
  value: string,
): Promise<void> {
  await settings.update({ [key]: value } as SettingsUpdate);
}

async function commitToggle(
  key:
    | "launchAtLogin"
    | "privacyMode"
    | "showServiceStatus"
    | "checkUpdatesOnStart"
    | "verboseLogging",
  checked: boolean,
): Promise<void> {
  await settings.update({ [key]: checked });
}

async function commitHudEnabled(enabled: boolean): Promise<void> {
  if (!current.value) return;
  await settings.update({ hud: { ...current.value.hud, enabled } });
}

function serviceValue(service: QuotaService, field: keyof ServiceSettings): boolean {
  return current.value?.services[service][field] ?? false;
}

async function commitService(
  service: QuotaService,
  field: keyof ServiceSettings,
  checked: boolean,
): Promise<void> {
  const services = current.value?.services;
  if (!services) return;
  const next: ServicesSettings = structuredClone(services);
  next[service][field] = checked;
  await settings.update({ services: next });
}

async function commitLocalAgentStats(checked: boolean): Promise<void> {
  const services = current.value?.services;
  if (!services) return;
  await settings.update({ services: { ...structuredClone(services), localAgentStats: checked } });
}

// ---- 服务行状态：可用性、邮箱、套餐，取自现有额度快照 ----
const credentialSources = ref<CredentialSources | null>(null);

function serviceStatusText(service: QuotaService): string {
  const snapshot = quota.ordered.find(
    (provider) => provider.provider === service && provider.kind === "primary",
  );
  if (!snapshot) return "";
  const parts: string[] = [];
  if (snapshot.identity?.account) parts.push(privacy.account(snapshot.identity.account));
  if (snapshot.identity?.plan) parts.push(snapshot.identity.plan);
  parts.push(t(presentProvider(snapshot).titleKey));
  return parts.join(" · ");
}

/** Claude 的凭据来源：Claude Code（文件／钥匙串）与 Claude Desktop 是否可用。 */
const claudeSourceText = computed(() => {
  const sources = credentialSources.value;
  if (!sources) return "";
  const code = t(`settings.credentialSource.claudeCode.${sources.claudeCode}`);
  const desktop = t(
    sources.claudeDesktop
      ? "settings.credentialSource.desktopAvailable"
      : "settings.credentialSource.desktopMissing",
  );
  return `${code} · ${desktop}`;
});

const commandCodeSourceText = computed(() => {
  const source = credentialSources.value?.commandCode;
  return source ? t(`settings.credentialSource.commandCode.${source}`) : "";
});

// ---- Codex 副账号 ----
const codexAccounts = ref<ImportedCodexAccount[]>([]);
const importPayload = ref("");
const importAlias = ref("");
const importPending = ref(false);
const accountMessage = ref<{ key: string; error: boolean } | null>(null);
const renamingId = ref<string | null>(null);
const renameDraft = ref("");

const ACCOUNT_ERROR_KEY: Record<string, string> = {
  codexAccountInvalid: "settings.accountError.invalid",
  codexAccountUnreadable: "settings.accountError.unreadable",
  codexAccountRejected: "settings.accountError.rejected",
  codexAccountOffline: "settings.accountError.offline",
  codexAccountRateLimited: "settings.accountError.rateLimited",
  codexAccountUnidentified: "settings.accountError.unidentified",
  codexAccountStoreFailed: "settings.accountError.storeFailed",
  credentialStoreFailed: "settings.accountError.storeFailed",
};

function accountName(account: ImportedCodexAccount): string {
  return account.displayName.includes("@")
    ? privacy.account(account.displayName)
    : account.displayName;
}

function reportAccountError(error: unknown): void {
  accountMessage.value = {
    key: ACCOUNT_ERROR_KEY[commandErrorCode(error)] ?? "settings.accountError.unknown",
    error: true,
  };
}

async function loadAccounts(): Promise<void> {
  try {
    codexAccounts.value = await getCodexAccounts();
  } catch {
    codexAccounts.value = [];
  }
}

async function runAccountAction(action: () => Promise<ImportedCodexAccount[]>): Promise<boolean> {
  try {
    codexAccounts.value = await action();
    return true;
  } catch (error) {
    reportAccountError(error);
    return false;
  }
}

async function importAccount(): Promise<void> {
  if (importPending.value || !importPayload.value.trim()) return;
  importPending.value = true;
  accountMessage.value = null;
  const ok = await runAccountAction(() =>
    importCodexAccount(importPayload.value, importAlias.value.trim() || null),
  );
  importPending.value = false;
  if (ok) {
    // 凭据已交给 Rust，输入框立即清空，不在界面里留着令牌原文。
    importPayload.value = "";
    importAlias.value = "";
    accountMessage.value = { key: "settings.accountImported", error: false };
  }
}

function startRename(account: ImportedCodexAccount): void {
  renamingId.value = account.id;
  renameDraft.value = account.displayName.includes("@") ? "" : account.displayName;
}

async function commitRename(account: ImportedCodexAccount): Promise<void> {
  const alias = renameDraft.value.trim();
  renamingId.value = null;
  accountMessage.value = null;
  await runAccountAction(() => updateCodexAccount(account.id, { alias }));
}

async function toggleAccountVisible(account: ImportedCodexAccount): Promise<void> {
  accountMessage.value = null;
  await runAccountAction(() => updateCodexAccount(account.id, { visible: !account.visible }));
}

async function moveAccount(index: number, delta: -1 | 1): Promise<void> {
  const ids = codexAccounts.value.map((account) => account.id);
  const target = index + delta;
  if (target < 0 || target >= ids.length) return;
  [ids[index], ids[target]] = [ids[target], ids[index]];
  accountMessage.value = null;
  await runAccountAction(() => reorderCodexAccounts(ids));
}

const removeConfirmId = ref<string | null>(null);
async function removeAccount(account: ImportedCodexAccount): Promise<void> {
  if (removeConfirmId.value !== account.id) {
    removeConfirmId.value = account.id;
    return;
  }
  removeConfirmId.value = null;
  accountMessage.value = null;
  await runAccountAction(() => removeCodexAccount(account.id));
}

// ---- Command Code API Key ----
const commandCode = ref<CommandCodeCredentialState | null>(null);
const commandCodeKey = ref("");
const commandCodeMessage = ref<{ key: string; error: boolean } | null>(null);

async function loadCommandCode(): Promise<void> {
  try {
    commandCode.value = await getCommandCodeCredentialState();
  } catch {
    commandCode.value = null;
  }
}

async function saveCommandCodeKey(): Promise<void> {
  if (!commandCodeKey.value.trim()) return;
  commandCodeMessage.value = null;
  try {
    commandCode.value = await setCommandCodeApiKey(commandCodeKey.value.trim());
    commandCodeKey.value = "";
    commandCodeMessage.value = { key: "settings.commandCodeSaved", error: false };
  } catch (error) {
    commandCodeMessage.value = {
      key:
        commandErrorCode(error) === "invalidArgument"
          ? "settings.commandCodeInvalid"
          : "settings.accountError.storeFailed",
      error: true,
    };
  }
}

async function clearCommandCodeKey(): Promise<void> {
  commandCodeMessage.value = null;
  try {
    commandCode.value = await clearCommandCodeApiKey();
    commandCodeMessage.value = { key: "settings.commandCodeCleared", error: false };
  } catch {
    commandCodeMessage.value = { key: "settings.accountError.storeFailed", error: true };
  }
}

// ---- 诊断与更新 ----
const diagnosticsState = ref<"idle" | "pending" | "saved" | "cancelled" | "failed">("idle");
const logFolderFailed = ref(false);

async function runDiagnosticsExport(): Promise<void> {
  if (diagnosticsState.value === "pending") return;
  diagnosticsState.value = "pending";
  try {
    diagnosticsState.value = await exportDiagnostics();
  } catch {
    diagnosticsState.value = "failed";
  }
}

async function openLogFolder(): Promise<void> {
  try {
    logFolderFailed.value = !(await revealLogFolder());
  } catch {
    logFolderFailed.value = true;
  }
}

const updateStatus = ref<UpdateStatus>({ state: "idle" });
const updatePending = ref(false);
let stopUpdateListener: (() => void) | null = null;

const updateText = computed(() => {
  const status = updateStatus.value;
  switch (status.state) {
    case "upToDate":
      return t("update.upToDate", { version: status.latestVersion });
    case "available":
      return t("update.available", { version: status.latestVersion });
    case "failed":
      return t("update.failed");
    case "rateLimited":
      return t("update.rateLimited");
    default:
      return "";
  }
});

async function runUpdateCheck(): Promise<void> {
  if (updatePending.value) return;
  if (updateStatus.value.state === "available") {
    await openReleasePage();
    return;
  }
  updatePending.value = true;
  try {
    updateStatus.value = await checkForUpdates();
  } catch {
    updateStatus.value = { state: "failed" };
  } finally {
    updatePending.value = false;
  }
}

const updateButtonLabel = computed(() =>
  updatePending.value
    ? t("update.checking")
    : updateStatus.value.state === "available"
      ? t("update.download")
      : updateStatus.value.state === "failed" || updateStatus.value.state === "rateLimited"
        ? t("update.retry")
        : t("update.check"),
);

onMounted(() => {
  void loadAccounts();
  void loadCommandCode();
  void getCredentialSources()
    .then((sources) => {
      credentialSources.value = sources;
    })
    .catch(() => undefined);
  void getUpdateStatus()
    .then((status) => {
      updateStatus.value = status;
    })
    .catch(() => undefined);
  void onUpdateStatus((status) => {
    updateStatus.value = status;
  })
    .then((stop) => {
      stopUpdateListener = stop;
    })
    .catch(() => undefined);
});

onUnmounted(() => {
  clearRebuildPoll();
  if (rebuildConfirmTimer) clearTimeout(rebuildConfirmTimer);
  stopUpdateListener?.();
});
</script>

<template>
  <main class="settings" :aria-label="t('a11y.settingsRegion')">
    <div class="settings__inner">
      <header class="settings__header">
        <h1 id="main-settings-title" tabindex="-1">{{ t("settings.title") }}</h1>
        <div class="sw-tabs" role="tablist" :aria-label="t('settings.title')">
          <button
            v-for="item in CATEGORIES"
            :key="item"
            type="button"
            role="tab"
            :data-category="item"
            :class="{ on: category === item }"
            :aria-selected="category === item"
            @click="category = item"
          >
            {{ t(`settings.category.${item}`) }}
          </button>
        </div>
      </header>

      <template v-if="current">
        <p v-if="settings.writeFailed" class="settings__error" role="alert">
          <strong>{{ t("error.settingsWriteFailed.title") }}</strong>
          <span>{{ t("error.settingsWriteFailed.nextStep") }}</span>
        </p>

        <!-- 服务与账号 -->
        <template v-if="category === 'services'">
          <section class="sw-group">
            <h2>{{ t("settings.servicesTitle") }}</h2>
            <p class="supporting settings__group-description">
              {{ t("settings.servicesDescription") }}
            </p>
            <div class="card sw-card">
              <div class="sw-matrix sw-matrix--head" aria-hidden="true">
                <span />
                <span v-for="field in SERVICE_FIELDS" :key="field" class="sw-cell">
                  {{ t(`settings.matrix.${field}`) }}
                </span>
              </div>
              <div v-for="service in QUOTA_SERVICES" :key="service" class="sw-matrix">
                <span class="sw-label">
                  <span translate="no">{{ t(`provider.${service}`) }}</span>
                  <span v-if="serviceStatusText(service)" class="sw-desc">
                    {{ serviceStatusText(service) }}
                  </span>
                  <span v-if="service === 'claude' && claudeSourceText" class="sw-desc">
                    {{ claudeSourceText }}
                  </span>
                  <span v-if="service === 'commandCode' && commandCodeSourceText" class="sw-desc">
                    {{ commandCodeSourceText }}
                  </span>
                </span>
                <button
                  v-for="field in SERVICE_FIELDS"
                  :key="field"
                  type="button"
                  class="toggle"
                  :class="{ off: !serviceValue(service, field) }"
                  role="switch"
                  :aria-checked="serviceValue(service, field)"
                  :aria-label="`${t(`provider.${service}`)} · ${t(`settings.matrix.${field}`)}`"
                  :disabled="field === 'hud' && !current.hud.enabled"
                  @click="commitService(service, field, !serviceValue(service, field))"
                />
              </div>
              <div class="sw-matrix">
                <span class="sw-label">
                  {{ t("settings.localAgentSources") }}
                  <span class="sw-desc">{{ t("settings.localAgentSourcesDescription") }}</span>
                </span>
                <span class="sw-cell" />
                <span class="sw-cell" />
                <span class="sw-cell" />
                <button
                  type="button"
                  class="toggle"
                  :class="{ off: !current.services.localAgentStats }"
                  role="switch"
                  :aria-checked="current.services.localAgentStats"
                  :aria-label="t('settings.localAgentSources')"
                  @click="commitLocalAgentStats(!current.services.localAgentStats)"
                />
              </div>
            </div>
          </section>

          <section class="sw-group">
            <h2>{{ t("settings.codexAccountsTitle") }}</h2>
            <p class="supporting settings__group-description">
              {{ t("settings.codexAccountsDescription") }}
            </p>
            <div class="card sw-card">
              <div v-for="(account, index) in codexAccounts" :key="account.id" class="sw-account">
                <span class="sw-label">
                  <template v-if="renamingId === account.id">
                    <input
                      v-model="renameDraft"
                      type="text"
                      :aria-label="t('settings.accountAlias')"
                      autocomplete="off"
                      @keydown.enter="commitRename(account)"
                      @keydown.esc="renamingId = null"
                    />
                  </template>
                  <template v-else>
                    <span translate="no">{{ accountName(account) }}</span>
                  </template>
                  <span class="sw-desc">
                    <template v-if="account.email && !account.displayName.includes('@')">
                      {{ privacy.account(account.email) }} ·
                    </template>
                    {{ account.plan ?? "—" }}
                    <template v-if="account.personalAccessToken"> · PAT</template>
                  </span>
                </span>
                <span class="sw-account__actions">
                  <button
                    type="button"
                    class="flat-btn"
                    :disabled="index === 0"
                    :aria-label="t('settings.accountMoveUp')"
                    @click="moveAccount(index, -1)"
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    class="flat-btn"
                    :disabled="index === codexAccounts.length - 1"
                    :aria-label="t('settings.accountMoveDown')"
                    @click="moveAccount(index, 1)"
                  >
                    ↓
                  </button>
                  <button
                    v-if="renamingId === account.id"
                    type="button"
                    class="flat-btn"
                    @click="commitRename(account)"
                  >
                    {{ t("settings.accountSave") }}
                  </button>
                  <button v-else type="button" class="flat-btn" @click="startRename(account)">
                    {{ t("settings.accountRename") }}
                  </button>
                  <button type="button" class="flat-btn" @click="toggleAccountVisible(account)">
                    {{ account.visible ? t("settings.accountHide") : t("settings.accountShow") }}
                  </button>
                  <button
                    type="button"
                    class="flat-btn"
                    :class="{ 'flat-btn--danger': removeConfirmId === account.id }"
                    @click="removeAccount(account)"
                  >
                    {{
                      removeConfirmId === account.id
                        ? t("settings.accountRemoveConfirm")
                        : t("settings.accountRemove")
                    }}
                  </button>
                </span>
              </div>
              <p v-if="codexAccounts.length === 0" class="sw-desc settings__action-status">
                {{ t("settings.accountsEmpty") }}
              </p>
              <form class="sw-form" @submit.prevent="importAccount">
                <textarea
                  v-model="importPayload"
                  name="codex-import"
                  spellcheck="false"
                  autocomplete="off"
                  :placeholder="t('settings.accountImportPlaceholder')"
                  :aria-label="t('settings.accountImportPlaceholder')"
                />
                <input
                  v-model="importAlias"
                  type="text"
                  name="codex-alias"
                  autocomplete="off"
                  :placeholder="t('settings.accountAlias')"
                  :aria-label="t('settings.accountAlias')"
                />
                <div class="sw-form__actions">
                  <button
                    type="submit"
                    class="flat-btn"
                    :disabled="importPending || !importPayload.trim()"
                  >
                    {{
                      importPending ? t("settings.accountImporting") : t("settings.accountImport")
                    }}
                  </button>
                  <span class="sw-desc">{{ t("settings.accountImportHint") }}</span>
                </div>
              </form>
              <p
                v-if="accountMessage"
                class="settings__action-status supporting"
                :class="{ 'settings__action-status--error': accountMessage.error }"
                aria-live="polite"
              >
                {{ t(accountMessage.key) }}
              </p>
            </div>
          </section>

          <section class="sw-group">
            <h2>{{ t("settings.commandCodeTitle") }}</h2>
            <p class="supporting settings__group-description">
              {{ t("settings.commandCodeDescription") }}
            </p>
            <div class="card sw-card">
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.commandCodeSource") }}
                  <span class="sw-desc">
                    {{
                      commandCode?.hasManualKey
                        ? t("settings.commandCodeManual")
                        : t("settings.commandCodeAutomatic")
                    }}
                  </span>
                </span>
                <button
                  v-if="commandCode?.hasManualKey"
                  type="button"
                  class="flat-btn"
                  @click="clearCommandCodeKey"
                >
                  {{ t("settings.commandCodeClear") }}
                </button>
              </div>
              <form class="sw-form" @submit.prevent="saveCommandCodeKey">
                <input
                  v-model="commandCodeKey"
                  type="password"
                  name="command-code-key"
                  autocomplete="off"
                  spellcheck="false"
                  :placeholder="t('settings.commandCodeKeyPlaceholder')"
                  :aria-label="t('settings.commandCodeKeyPlaceholder')"
                />
                <div class="sw-form__actions">
                  <button type="submit" class="flat-btn" :disabled="!commandCodeKey.trim()">
                    {{ t("settings.commandCodeSave") }}
                  </button>
                </div>
              </form>
              <p
                v-if="commandCodeMessage"
                class="settings__action-status supporting"
                :class="{ 'settings__action-status--error': commandCodeMessage.error }"
                aria-live="polite"
              >
                {{ t(commandCodeMessage.key) }}
              </p>
            </div>
          </section>
        </template>

        <!-- 外观与显示 -->
        <template v-else-if="category === 'appearance'">
          <section class="sw-group">
            <h2>{{ t("settings.menuBar") }}</h2>
            <div class="card sw-card">
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.menuBarMode") }}
                  <span class="sw-desc">{{ t("settings.menuBarModeDescription") }}</span>
                </span>
                <div class="segmented" role="group" :aria-label="t('settings.menuBarMode')">
                  <button
                    v-for="option in MENU_BAR_WINDOW_MODE_OPTIONS"
                    :key="option"
                    type="button"
                    :class="{ on: current.menuBarWindowMode === option }"
                    :aria-pressed="current.menuBarWindowMode === option"
                    @click="commitSelect('menuBarWindowMode', option)"
                  >
                    {{ t(MENU_BAR_MODE_LABEL[option]) }}
                  </button>
                </div>
              </div>
            </div>
          </section>
          <section class="sw-group">
            <h2>{{ t("settings.hud") }}</h2>
            <div class="card sw-card">
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.hudEnabled") }}
                  <span class="sw-desc">{{ t("settings.hudEnabledDescription") }}</span>
                </span>
                <button
                  type="button"
                  class="toggle"
                  :class="{ off: !current.hud.enabled }"
                  role="switch"
                  :aria-checked="current.hud.enabled"
                  @click="commitHudEnabled(!current.hud.enabled)"
                >
                  <span class="visually-hidden">{{ t("settings.hudEnabled") }}</span>
                </button>
              </div>
            </div>
          </section>
          <section class="sw-group">
            <h2>{{ t("settings.displayDetails") }}</h2>
            <div class="card sw-card">
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.resetTime") }}
                  <span class="sw-desc">{{ t("settings.resetTimeDescription") }}</span>
                </span>
                <div class="segmented" role="group" :aria-label="t('settings.resetTime')">
                  <button
                    v-for="option in RESET_TIME_DISPLAY_OPTIONS"
                    :key="option"
                    type="button"
                    :class="{ on: current.resetTimeDisplay === option }"
                    :aria-pressed="current.resetTimeDisplay === option"
                    @click="commitSelect('resetTimeDisplay', option)"
                  >
                    {{ t(RESET_TIME_LABEL[option]) }}
                  </button>
                </div>
              </div>
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.serviceStatus") }}
                  <span class="sw-desc">{{ t("settings.serviceStatusDescription") }}</span>
                </span>
                <button
                  type="button"
                  class="toggle"
                  :class="{ off: !current.showServiceStatus }"
                  role="switch"
                  :aria-checked="current.showServiceStatus"
                  @click="commitToggle('showServiceStatus', !current.showServiceStatus)"
                >
                  <span class="visually-hidden">{{ t("settings.serviceStatus") }}</span>
                </button>
              </div>
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.privacyMode") }}
                  <span class="sw-desc">{{ t("settings.privacyModeDescription") }}</span>
                </span>
                <button
                  type="button"
                  class="toggle"
                  :class="{ off: !current.privacyMode }"
                  role="switch"
                  :aria-checked="current.privacyMode"
                  @click="commitToggle('privacyMode', !current.privacyMode)"
                >
                  <span class="visually-hidden">{{ t("settings.privacyMode") }}</span>
                </button>
              </div>
            </div>
          </section>
          <section class="sw-group">
            <h2>{{ t("settings.statistics") }}</h2>
            <div class="card sw-card">
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.rankingBasis") }}
                  <span class="sw-desc">{{ t("settings.rankingBasisDescription") }}</span>
                </span>
                <div class="segmented" role="group" :aria-label="t('settings.rankingBasis')">
                  <button
                    v-for="option in RANKING_BASIS_OPTIONS"
                    :key="option"
                    type="button"
                    :class="{ on: current.rankingBasis === option }"
                    :aria-pressed="current.rankingBasis === option"
                    @click="commitSelect('rankingBasis', option)"
                  >
                    {{ t(RANKING_LABEL[option]) }}
                  </button>
                </div>
              </div>
            </div>
          </section>
        </template>

        <!-- 数据与刷新 -->
        <template v-else-if="category === 'data'">
          <section class="sw-group">
            <h2>{{ t("settings.polling") }}</h2>
            <div class="card sw-card">
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.refreshInterval") }}
                </span>
                <select
                  name="refresh-interval"
                  :value="current.refreshInterval"
                  autocomplete="off"
                  @change="
                    commitSelect('refreshInterval', ($event.target as HTMLSelectElement).value)
                  "
                >
                  <option v-for="option in REFRESH_INTERVAL_OPTIONS" :key="option" :value="option">
                    {{ t(INTERVAL_LABEL[option]) }}
                  </option>
                </select>
              </div>
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.scanInterval") }}
                  <span class="sw-desc">{{ t("settings.scanIntervalDescription") }}</span>
                </span>
                <select
                  name="scan-interval"
                  :value="current.scanInterval"
                  autocomplete="off"
                  @change="commitSelect('scanInterval', ($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="option in REFRESH_INTERVAL_OPTIONS" :key="option" :value="option">
                    {{ t(INTERVAL_LABEL[option]) }}
                  </option>
                </select>
              </div>
            </div>
          </section>
          <section class="sw-group">
            <h2>{{ t("settings.usageAndPricing") }}</h2>
            <div class="card sw-card">
              <div class="sw-row sw-row--action">
                <span class="sw-label">
                  {{ t("settings.pricingCatalog") }}
                  <span class="sw-desc">{{ t("settings.pricingCatalogDescription") }}</span>
                </span>
                <button
                  type="button"
                  class="flat-btn"
                  :disabled="pricingRefreshPending"
                  @click="updatePricingCatalog"
                >
                  {{
                    pricingRefreshPending
                      ? t("settings.pricingCatalogUpdating")
                      : t("settings.pricingCatalogUpdate")
                  }}
                </button>
              </div>
              <p
                v-if="pricingRefreshState !== 'idle'"
                class="settings__action-status supporting"
                :class="{ 'settings__action-status--error': pricingRefreshState === 'failure' }"
                aria-live="polite"
              >
                {{
                  pricingRefreshState === "success"
                    ? t("settings.pricingCatalogUpdated")
                    : pricingRefreshState === "partial"
                      ? t("settings.pricingCatalogPartiallyUpdated")
                      : t("settings.pricingCatalogUpdateFailed")
                }}
              </p>
              <div class="sw-row sw-row--action">
                <span class="sw-label">
                  {{ t("settings.rebuildUsage") }}
                  <span class="sw-desc">{{ t("settings.rebuildUsageDescription") }}</span>
                </span>
                <button
                  type="button"
                  class="flat-btn"
                  data-rebuild-btn
                  :class="{ 'flat-btn--danger': rebuildState === 'confirm' }"
                  :disabled="rebuildPending"
                  @click="requestRebuild"
                >
                  {{ rebuildLabel }}
                </button>
              </div>
              <p
                v-if="
                  rebuildState === 'running' ||
                  rebuildState === 'success' ||
                  rebuildState === 'failure'
                "
                class="settings__action-status supporting"
                :class="{ 'settings__action-status--error': rebuildState === 'failure' }"
                aria-live="polite"
              >
                {{ rebuildStatusText }}
              </p>
            </div>
          </section>
        </template>

        <!-- 通用 -->
        <template v-else>
          <section class="sw-group">
            <h2>{{ t("settings.system") }}</h2>
            <div class="card sw-card">
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.language") }}
                </span>
                <div class="segmented" role="group" :aria-label="t('settings.language')">
                  <button
                    v-for="option in LANGUAGE_OPTIONS"
                    :key="option"
                    type="button"
                    :class="{ on: current.language === option }"
                    :aria-pressed="current.language === option"
                    @click="commitSelect('language', option)"
                  >
                    {{ t(LANGUAGE_LABEL[option]) }}
                  </button>
                </div>
              </div>
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.appearance") }}
                </span>
                <div class="segmented" role="group" :aria-label="t('settings.appearance')">
                  <button
                    v-for="option in APPEARANCE_OPTIONS"
                    :key="option"
                    type="button"
                    :class="{ on: current.appearance === option }"
                    :aria-pressed="current.appearance === option"
                    @click="commitSelect('appearance', option)"
                  >
                    {{ t(APPEARANCE_LABEL[option]) }}
                  </button>
                </div>
              </div>
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("settings.launchAtLogin") }}
                </span>
                <button
                  type="button"
                  class="toggle"
                  :class="{ off: !current.launchAtLogin }"
                  role="switch"
                  :aria-checked="current.launchAtLogin"
                  @click="commitToggle('launchAtLogin', !current.launchAtLogin)"
                >
                  <span class="visually-hidden">{{ t("settings.launchAtLogin") }}</span>
                </button>
              </div>
            </div>
          </section>

          <section class="sw-group">
            <h2>{{ t("diagnostics.title") }}</h2>
            <div class="card sw-card">
              <div class="sw-row sw-row--action">
                <span class="sw-label">
                  {{ t("diagnostics.export") }}
                  <span class="sw-desc">{{ t("diagnostics.exportDescription") }}</span>
                </span>
                <button
                  type="button"
                  class="flat-btn"
                  data-diagnostics-export
                  :disabled="diagnosticsState === 'pending'"
                  @click="runDiagnosticsExport"
                >
                  {{
                    diagnosticsState === "pending"
                      ? t("diagnostics.exporting")
                      : t("diagnostics.exportAction")
                  }}
                </button>
              </div>
              <p
                v-if="diagnosticsState === 'saved' || diagnosticsState === 'failed'"
                class="settings__action-status supporting"
                :class="{ 'settings__action-status--error': diagnosticsState === 'failed' }"
                aria-live="polite"
              >
                {{
                  diagnosticsState === "saved" ? t("diagnostics.saved") : t("diagnostics.failed")
                }}
              </p>
              <div class="sw-row sw-row--action">
                <span class="sw-label">
                  {{ t("diagnostics.openLogs") }}
                  <span class="sw-desc">{{ t("diagnostics.openLogsDescription") }}</span>
                </span>
                <button type="button" class="flat-btn" @click="openLogFolder">
                  {{ t("diagnostics.openAction") }}
                </button>
              </div>
              <p
                v-if="logFolderFailed"
                class="settings__action-status supporting settings__action-status--error"
                aria-live="polite"
              >
                {{ t("diagnostics.openLogsFailed") }}
              </p>
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("diagnostics.verbose") }}
                  <span class="sw-desc">{{ t("diagnostics.verboseDescription") }}</span>
                </span>
                <button
                  type="button"
                  class="toggle"
                  :class="{ off: !current.verboseLogging }"
                  role="switch"
                  :aria-checked="current.verboseLogging"
                  @click="commitToggle('verboseLogging', !current.verboseLogging)"
                >
                  <span class="visually-hidden">{{ t("diagnostics.verbose") }}</span>
                </button>
              </div>
            </div>
          </section>

          <section class="sw-group">
            <h2>{{ t("update.title") }}</h2>
            <div class="card sw-card">
              <div class="sw-row">
                <span class="sw-label">{{ t("settings.versionLabel") }}</span>
                <span class="numeric supporting">{{ settings.version }}</span>
              </div>
              <div class="sw-row sw-row--action">
                <span class="sw-label">
                  {{ t("update.check") }}
                  <span class="sw-desc">{{ t("update.description") }}</span>
                </span>
                <button
                  type="button"
                  class="flat-btn"
                  data-update-btn
                  :disabled="updatePending"
                  @click="runUpdateCheck"
                >
                  {{ updateButtonLabel }}
                </button>
              </div>
              <p
                v-if="updateText"
                class="settings__action-status supporting"
                :class="{
                  'settings__action-status--error':
                    updateStatus.state === 'failed' || updateStatus.state === 'rateLimited',
                }"
                aria-live="polite"
              >
                {{ updateText }}
              </p>
              <div class="sw-row">
                <span class="sw-label">
                  {{ t("update.checkOnStart") }}
                </span>
                <button
                  type="button"
                  class="toggle"
                  :class="{ off: !current.checkUpdatesOnStart }"
                  role="switch"
                  :aria-checked="current.checkUpdatesOnStart"
                  @click="commitToggle('checkUpdatesOnStart', !current.checkUpdatesOnStart)"
                >
                  <span class="visually-hidden">{{ t("update.checkOnStart") }}</span>
                </button>
              </div>
            </div>
          </section>

          <section class="sw-group">
            <h2>{{ t("settings.about") }}</h2>
            <div class="card sw-card sw-about">
              <p class="settings__version numeric">
                {{ t("settings.version", { version: settings.version }) }}
              </p>
              <p class="settings__privacy supporting">{{ t("settings.privacy") }}</p>
            </div>
          </section>
        </template>
      </template>
    </div>
  </main>
</template>

<style scoped>
.settings {
  min-block-size: 100vh;
  padding: clamp(1.5rem, 4vw, 2.5rem);
  background: var(--surface-primary);
}

.settings__inner {
  display: grid;
  align-content: start;
  gap: var(--space-6);
  inline-size: min(100%, 40rem);
  margin-inline: auto;
}

.settings__header {
  display: grid;
  justify-items: start;
  gap: var(--space-4);
  padding-block-end: 0.75rem;
  margin-block-end: 0.5rem;
  border-block-end: 1px solid var(--border-subtle);
}

h1 {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 680;
  letter-spacing: -0.025em;
  line-height: 1.15;
}

h1[tabindex="-1"]:focus {
  outline: none;
}

.sw-group {
  display: grid;
  gap: var(--space-3);
}

.sw-group > h2 {
  margin: 0 0 0 0.125rem;
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 680;
  letter-spacing: 0.04em;
}

.sw-card {
  padding: 0.25rem 0;
  background: var(--surface-raised);
  border: 1px solid var(--border-hairline);
  border-radius: 0.875rem;
}

.sw-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.75rem 1rem;
  min-block-size: 3rem;
}

.sw-row + .sw-row {
  border-block-start: 1px solid color-mix(in srgb, var(--border-subtle) 65%, transparent);
}

.sw-row--action {
  align-items: start;
}

.sw-label {
  display: grid;
  gap: 0.25rem;
  font-size: 0.8125rem;
  font-weight: 550;
  line-height: 1.35;
}

.sw-desc {
  color: var(--text-secondary);
  font-size: 0.71875rem;
  font-weight: 400;
  line-height: 1.5;
  max-inline-size: 22.5rem;
}

select {
  min-block-size: 2rem;
  padding: 0 var(--space-2);
  color: var(--text-primary);
  background-color: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
}

.segmented {
  display: inline-flex;
  flex: none;
  padding: 0.1875rem;
  border-radius: 0.5625rem;
  background: var(--track-background);
}

.segmented button {
  min-block-size: 1.625rem;
  padding: 0 0.6875rem;
  border: 0;
  border-radius: 0.375rem;
  color: var(--text-secondary);
  background: transparent;
  font-size: 0.6875rem;
  white-space: nowrap;
}

.segmented button:hover {
  color: var(--text-primary);
}

.segmented button.on {
  color: var(--text-primary);
  background: var(--surface-raised);
  box-shadow: 0 1px 3px rgb(16 16 20 / 12%);
  font-weight: 570;
}

/* 40 × 24 开关（产物 toggle），off 态用轨道色 */
.toggle {
  position: relative;
  flex: none;
  inline-size: 2.5rem;
  block-size: 1.5rem;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: var(--status-success);
  cursor: pointer;
}

.toggle::after {
  content: "";
  position: absolute;
  inset-block-start: 0.125rem;
  inset-inline-end: 0.125rem;
  inline-size: 1.25rem;
  block-size: 1.25rem;
  border-radius: 50%;
  background: #ffffff;
  box-shadow: 0 1px 3px rgb(0 0 0 / 25%);
  transition: inset-inline-start var(--motion-fast) var(--ease-out);
}

.toggle.off {
  background: var(--track-background);
}

.toggle.off::after {
  inset-inline-end: auto;
  inset-inline-start: 0.125rem;
}

.toggle:hover {
  filter: brightness(1.05);
}

.flat-btn {
  flex: none;
  min-block-size: 2.25rem;
  padding: 0 0.875rem;
  border: 0;
  border-radius: 0.5625rem;
  color: var(--text-primary);
  background: var(--track-background);
  font-size: 0.75rem;
  font-weight: 570;
  white-space: nowrap;
}

.flat-btn:hover {
  background: color-mix(in srgb, var(--text-primary) 12%, var(--track-background));
}

.flat-btn--danger,
.flat-btn--danger:hover {
  color: var(--status-error);
  background: color-mix(in srgb, var(--status-error) 12%, var(--track-background));
}

.flat-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.settings__group-description {
  margin: 0;
  font-size: 0.71875rem;
  line-height: 1.5;
}

.settings__action-status {
  margin: 0;
  padding: 0 1rem 0.75rem;
  font-size: 0.71875rem;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.settings__action-status--error {
  color: var(--status-error);
}

.settings__error {
  display: grid;
  gap: var(--space-1);
  margin: 0;
  padding: var(--space-3) var(--space-4);
  color: var(--status-error);
  background: var(--surface-raised);
  border: 1px solid var(--status-error);
  border-radius: var(--radius-small);
  font-size: 0.8125rem;
}

.settings__error span {
  color: var(--text-secondary);
}

.sw-about {
  display: grid;
  gap: var(--space-2);
  padding: 0.75rem 1rem;
}

.settings__version {
  margin: 0;
  font-size: 0.8125rem;
}

.settings__privacy {
  margin: 0;
  font-size: 0.71875rem;
  line-height: 1.6;
}

.sw-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem;
}

.sw-tabs button {
  min-block-size: 2rem;
  padding: 0 0.875rem;
  border: 0;
  border-radius: 0.5625rem;
  color: var(--text-secondary);
  background: transparent;
  font-size: 0.75rem;
  font-weight: 570;
}

.sw-tabs button.on {
  color: var(--text-primary);
  background: var(--track-background);
}

.sw-matrix {
  display: grid;
  grid-template-columns: minmax(0, 1fr) repeat(4, 3.5rem);
  align-items: center;
  column-gap: 0.5rem;
  padding: 0.5rem 1rem;
}

.sw-matrix--head {
  color: var(--text-secondary);
  font-size: 0.6875rem;
  font-weight: 570;
}

.sw-matrix + .sw-matrix {
  border-block-start: 1px solid color-mix(in srgb, var(--border-subtle) 65%, transparent);
}

.sw-matrix > .toggle,
.sw-matrix > .sw-cell {
  justify-self: center;
}

.sw-cell {
  color: var(--text-secondary);
}

.toggle:disabled {
  opacity: 0.4;
  cursor: default;
}

.sw-account {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: 0.5rem 0.75rem;
  padding: 0.75rem 1rem;
}

.sw-account + .sw-account,
.sw-form {
  border-block-start: 1px solid color-mix(in srgb, var(--border-subtle) 65%, transparent);
}

.sw-account__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.375rem;
  justify-content: flex-end;
}

.sw-form {
  display: grid;
  gap: 0.5rem;
  padding: 0.75rem 1rem;
}

.sw-form input,
.sw-form textarea,
.sw-account input {
  inline-size: 100%;
  padding: 0.375rem 0.5rem;
  color: var(--text-primary);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
  font: inherit;
  font-size: 0.75rem;
}

.sw-form textarea {
  min-block-size: 4.5rem;
  resize: vertical;
}

.sw-form__actions {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}

@media (prefers-reduced-motion: no-preference) {
  .segmented button,
  .toggle,
  .flat-btn {
    transition:
      background-color var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      filter var(--motion-fast) var(--ease-out),
      scale var(--motion-fast) var(--ease-out);
  }

  .segmented button:active,
  .toggle:active,
  .flat-btn:active {
    scale: 0.96;
  }
}
</style>
