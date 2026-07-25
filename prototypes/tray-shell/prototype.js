const scenarios = {
  live: {
    title: "额度状态正常",
    meta: "刚刚刷新",
    explanation: "当前数据来自本次成功刷新，可以用于判断额度风险。",
    providers: [
      { name: "Codex", value: 73, reset: "4小时12分后重置", state: "live", detail: "实时数据" },
      { name: "Claude Code", value: 38, reset: "周一 08:00 重置", state: "live", detail: "实时数据" },
    ],
  },
  loading: {
    title: "正在刷新额度…",
    meta: "保留上一份有效快照",
    explanation: "刷新期间继续显示已有额度；两个 Provider 独立完成请求。",
    providers: [
      { name: "Codex", value: 73, reset: "4小时12分后重置", state: "live", detail: "正在刷新…" },
      { name: "Claude Code", value: 38, reset: "周一 08:00 重置", state: "live", detail: "正在刷新…" },
    ],
  },
  noCredentials: {
    title: "尚未发现可用凭据",
    meta: "可在对应 CLI 登录后刷新",
    explanation: "CC Trace 不在应用内登录，也不会要求粘贴 Token。",
    providers: [
      { name: "Codex", value: null, reset: "检查 Codex CLI 登录状态", state: "empty", detail: "无凭据" },
      { name: "Claude Code", value: null, reset: "检查 Claude Code 登录状态", state: "empty", detail: "无凭据" },
    ],
  },
  offlineStale: {
    title: "当前离线，显示旧快照",
    meta: "上次成功：今天 10:42",
    explanation: "额度值来自上一次成功刷新，网络恢复后可再次刷新。",
    providers: [
      { name: "Codex", value: 61, reset: "约3小时后重置", state: "warning", detail: "离线 · 旧快照" },
      { name: "Claude Code", value: 27, reset: "周一 08:00 重置", state: "warning", detail: "离线 · 旧快照" },
    ],
  },
  rateLimited: {
    title: "Codex 刷新受限",
    meta: "Claude Code 已正常更新",
    explanation: "Codex 暂时停止请求并保留旧快照；12 分钟后可再次尝试。",
    providers: [
      { name: "Codex", value: 55, reset: "12分钟后可重试", state: "warning", detail: "刷新受限 · 旧快照" },
      { name: "Claude Code", value: 38, reset: "周一 08:00 重置", state: "live", detail: "实时数据" },
    ],
  },
  error: {
    title: "Codex 需要处理",
    meta: "Claude Code 不受影响",
    explanation: "Codex 凭据不可用；请检查对应 CLI 登录状态。Claude Code 仍为实时数据。",
    providers: [
      { name: "Codex", value: null, reset: "检查 Codex CLI 登录状态", state: "error", detail: "凭据不可用" },
      { name: "Claude Code", value: 38, reset: "周一 08:00 重置", state: "live", detail: "实时数据" },
    ],
  },
};

const providerList = document.querySelector("#providerList");
const mainProviders = document.querySelector("#mainProviders");
const scenarioSelect = document.querySelector("#scenario");
const prototype = document.querySelector("#prototype");
const compactPanel = document.querySelector("#compactPanel");
const trayButton = document.querySelector("#trayButton");
const trayMenu = document.querySelector("#trayMenu");

function providerMarkup(provider, detailed = false) {
  const value = provider.value === null ? "—" : `${provider.value}%`;
  const remaining = provider.value === null ? 0 : provider.value;
  const stateClass =
    provider.state === "error"
      ? "state-error"
      : provider.state === "warning"
        ? "state-warning"
        : "state-live";
  const ariaValue =
    provider.value === null
      ? 'aria-valuetext="无可用额度"'
      : `aria-valuenow="${remaining}" aria-valuetext="剩余 ${remaining}%"`;

  return `
    <article class="provider-lane ${stateClass}">
      <div class="provider-heading">
        <strong>${provider.name}</strong>
        <span class="supporting">${provider.detail}</span>
      </div>
      <div class="rail-labels">
        <span class="quota-value">${value}</span>
        <span class="reset-time">${provider.reset}</span>
      </div>
      <div
        class="reset-rail"
        role="progressbar"
        aria-label="${provider.name} 剩余额度"
        aria-valuemin="0"
        aria-valuemax="100"
        ${ariaValue}
        style="--remaining: ${remaining}%"
      ><span></span></div>
      <div class="status-line">
        <span>${detailed ? "主要额度" : "当前窗口"}</span>
        <span>${provider.detail}</span>
      </div>
    </article>
  `;
}

function renderScenario(name) {
  const scenario = scenarios[name];
  providerList.setAttribute("aria-busy", String(name === "loading"));
  document.querySelector("#overallTitle").textContent = scenario.title;
  document.querySelector("#overallMeta").textContent = scenario.meta;
  document.querySelector("#explanationCopy").textContent = scenario.explanation;
  providerList.innerHTML = scenario.providers.map((item) => providerMarkup(item)).join("");
  mainProviders.innerHTML = scenario.providers
    .map((item) => providerMarkup(item, true))
    .join("");
}

function openDialog(id) {
  const dialog = document.querySelector(`#${id}`);
  if (!dialog.open) dialog.showModal();
}

document.querySelector("#platform").addEventListener("change", (event) => {
  const isMac = event.target.value === "mac";
  closeTransientPanels();
  prototype.dataset.platform = event.target.value;
  document.querySelector("#systemLabel").textContent = isMac
    ? "macOS Menu Bar"
    : "Windows System Tray";
});

scenarioSelect.addEventListener("change", (event) => renderScenario(event.target.value));

document.querySelector("#theme").addEventListener("change", (event) => {
  prototype.dataset.theme = event.target.value;
  document.body.dataset.theme = event.target.value;
});

trayButton.addEventListener("click", () => {
  trayMenu.hidden = true;
  compactPanel.hidden = !compactPanel.hidden;
  trayButton.setAttribute("aria-expanded", String(!compactPanel.hidden));
});

trayButton.addEventListener("contextmenu", (event) => {
  if (prototype.dataset.platform !== "windows") return;
  event.preventDefault();
  compactPanel.hidden = true;
  trayButton.setAttribute("aria-expanded", "false");
  trayMenu.hidden = false;
  trayMenu.querySelector("button").focus();
});

document.querySelector("#refreshButton").addEventListener("click", () => {
  scenarioSelect.value = "loading";
  renderScenario("loading");
});

document.querySelector("#mainRefresh").addEventListener("click", () => {
  scenarioSelect.value = "loading";
  renderScenario("loading");
});

function closeTransientPanels() {
  compactPanel.hidden = true;
  trayMenu.hidden = true;
  trayButton.setAttribute("aria-expanded", "false");
}

function openMainWindow() {
  closeTransientPanels();
  openDialog("mainWindow");
}

function openSettingsWindow() {
  closeTransientPanels();
  openDialog("settingsWindow");
}

document.querySelector("#openMain").addEventListener("click", openMainWindow);
document.querySelector("#openSettings").addEventListener("click", openSettingsWindow);
document.querySelector("#showOnboarding").addEventListener("click", () => openDialog("onboardingWindow"));
document.querySelector("#finishOnboarding").addEventListener("click", () => {
  compactPanel.hidden = false;
  trayButton.setAttribute("aria-expanded", "true");
});
document.querySelector("#menuOpenMain").addEventListener("click", openMainWindow);
document.querySelector("#menuOpenSettings").addEventListener("click", openSettingsWindow);
document.querySelector("#menuRefresh").addEventListener("click", () => {
  closeTransientPanels();
  scenarioSelect.value = "loading";
  renderScenario("loading");
});

document.querySelector("#quitPrototype").addEventListener("click", () => {
  closeTransientPanels();
});
document.querySelector("#menuQuit").addEventListener("click", closeTransientPanels);

document.querySelectorAll("[data-close]").forEach((button) => {
  button.addEventListener("click", () => {
    document.querySelector(`#${button.dataset.close}`).close();
  });
});

document.addEventListener("pointerdown", (event) => {
  if (
    !trayMenu.hidden &&
    !trayMenu.contains(event.target) &&
    !trayButton.contains(event.target)
  ) {
    trayMenu.hidden = true;
  }

  if (
    compactPanel.hidden ||
    compactPanel.contains(event.target) ||
    trayButton.contains(event.target)
  ) {
    return;
  }
  compactPanel.hidden = true;
  trayButton.setAttribute("aria-expanded", "false");
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !compactPanel.hidden) {
    compactPanel.hidden = true;
    trayButton.setAttribute("aria-expanded", "false");
    trayButton.focus();
  }
});

renderScenario("live");
