/**
 * 隐私模式遮挡函数（仅展示层）。
 *
 * 遮挡只改变界面显示，不改变存储、请求或复制内容以外的数据；
 * 各视图在渲染前调用这里的函数，不要在组件里自己拼遮挡规则。
 * 每个函数接受 `enabled` 参数：为 false 时原样返回，便于视图统一写成
 * `maskPath(path, privacy.enabled.value)`。
 */
import { computed, type ComputedRef } from "vue";

import { useSettingsStore } from "../features/settings/store";

const MASK = "•••";

/** 路径：只保留末段（同时兼容 `/` 与 `\`），其余前缀用省略号替代。 */
export function maskPath(path: string | null | undefined, enabled = true): string {
  if (!path) return "";
  if (!enabled) return path;
  const trimmed = path.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/).filter(Boolean);
  const last = parts[parts.length - 1];
  if (!last) return MASK;
  return parts.length > 1 ? `…/${last}` : last;
}

/** 邮箱或账号名：保留首字符与邮箱域名，其余以圆点替代；无 `@` 时只保留首字符。 */
export function maskAccount(account: string | null | undefined, enabled = true): string {
  if (!account) return "";
  if (!enabled) return account;
  const at = account.indexOf("@");
  if (at > 0) {
    const local = account.slice(0, at);
    const domain = account.slice(at + 1);
    return `${Array.from(local)[0]}${MASK}@${domain}`;
  }
  const first = Array.from(account)[0] ?? "";
  return `${first}${MASK}`;
}

/** 分支名：整体遮挡，只保留占位。 */
export function maskBranch(branch: string | null | undefined, enabled = true): string {
  if (!branch) return "";
  return enabled ? MASK : branch;
}

/** 对话标题：整体遮挡，只保留占位。 */
export function maskTitle(title: string | null | undefined, enabled = true): string {
  if (!title) return "";
  return enabled ? MASK : title;
}

export interface PrivacyHelpers {
  enabled: ComputedRef<boolean>;
  path: (value: string | null | undefined) => string;
  account: (value: string | null | undefined) => string;
  branch: (value: string | null | undefined) => string;
  title: (value: string | null | undefined) => string;
}

/** 读取设置中的 `privacyMode`，并返回已绑定开关的遮挡函数。 */
export function usePrivacy(): PrivacyHelpers {
  const store = useSettingsStore();
  const enabled = computed(() => store.settings?.privacyMode ?? false);
  return {
    enabled,
    path: (value) => maskPath(value, enabled.value),
    account: (value) => maskAccount(value, enabled.value),
    branch: (value) => maskBranch(value, enabled.value),
    title: (value) => maskTitle(value, enabled.value),
  };
}
