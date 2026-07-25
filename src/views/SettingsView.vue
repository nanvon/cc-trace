<template>
  <main class="settings-shell">
    <header>
      <p class="utility-label">独立窗口 · 桌面壳</p>
      <h1>设置</h1>
    </header>

    <fieldset>
      <legend>通用</legend>
      <label>
        <span>自动刷新间隔</span>
        <select name="refreshInterval" autocomplete="off">
          <option>15 分钟</option>
          <option>30 分钟</option>
          <option>60 分钟</option>
        </select>
      </label>
      <label class="check-row">
        <input type="checkbox" name="launchAtLogin" />
        <span>开机时启动 CC Trace</span>
      </label>
    </fieldset>

    <fieldset>
      <legend>外观与语言</legend>
      <label>
        <span>语言</span>
        <select name="language" autocomplete="off">
          <option>跟随系统</option>
          <option>中文</option>
          <option>English</option>
        </select>
      </label>
      <label>
        <span>外观</span>
        <select name="appearance" autocomplete="off">
          <option>跟随系统</option>
          <option>浅色</option>
          <option>深色</option>
        </select>
      </label>
    </fieldset>

    <p class="about">
      CC Trace 0.1.0 · 当前设置不持久化，也不读取或迁移 cc-bar 应用数据。
    </p>
  </main>
</template>

<style scoped>
.settings-shell {
  min-height: 100vh;
  padding: var(--space-6);
  background: var(--surface-primary);
}

h1 {
  margin: 0;
  font-size: 1.8rem;
  letter-spacing: -0.035em;
}

fieldset {
  display: grid;
  gap: var(--space-4);
  margin: var(--space-6) 0 0;
  padding: var(--space-5) 0 0;
  border: 0;
  border-top: 1px solid var(--border-subtle);
}

legend {
  padding: 0;
  font-weight: 650;
}

label:not(.check-row) {
  display: grid;
  grid-template-columns: 1fr minmax(10rem, auto);
  align-items: center;
  gap: var(--space-4);
}

select {
  min-height: 2.5rem;
  padding: 0 var(--space-3);
  color: var(--text-primary);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
}

.check-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: 2.5rem;
}

.about {
  margin: var(--space-6) 0 0;
  color: var(--text-secondary);
  font-size: 0.8rem;
  line-height: 1.6;
}
</style>
