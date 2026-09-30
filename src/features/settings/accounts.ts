/**
 * 账号与凭据管理的前端 API。与 `src-tauri/src/commands/{codex_accounts,command_code}.rs` 一一对应。
 * 载荷永远不含 token 或 `auth.json` 原文。
 */
import { invoke } from "@tauri-apps/api/core";

import type { CommandCodeCredentialState } from "./contracts";

export interface ImportedCodexAccount {
  /** 复合身份，增删改的操作键。 */
  id: string;
  displayName: string;
  email: string | null;
  plan: string | null;
  visible: boolean;
  personalAccessToken: boolean;
  addedAt: string;
}

export function getCodexAccounts(): Promise<ImportedCodexAccount[]> {
  return invoke<ImportedCodexAccount[]>("codex_accounts_get");
}

/** 粘贴 `auth.json` 内容或 personal access token；返回更新后的完整列表。 */
export function importCodexAccount(
  payload: string,
  alias: string | null,
): Promise<ImportedCodexAccount[]> {
  return invoke<ImportedCodexAccount[]>("codex_accounts_import", { payload, alias });
}

export function updateCodexAccount(
  id: string,
  patch: { alias?: string; visible?: boolean },
): Promise<ImportedCodexAccount[]> {
  return invoke<ImportedCodexAccount[]>("codex_accounts_update", {
    id,
    alias: patch.alias ?? null,
    visible: patch.visible ?? null,
  });
}

export function removeCodexAccount(id: string): Promise<ImportedCodexAccount[]> {
  return invoke<ImportedCodexAccount[]>("codex_accounts_remove", { id });
}

export function reorderCodexAccounts(ids: string[]): Promise<ImportedCodexAccount[]> {
  return invoke<ImportedCodexAccount[]>("codex_accounts_reorder", { ids });
}

export function getCommandCodeCredentialState(): Promise<CommandCodeCredentialState> {
  return invoke<CommandCodeCredentialState>("command_code_credential_state");
}

export function setCommandCodeApiKey(apiKey: string): Promise<CommandCodeCredentialState> {
  return invoke<CommandCodeCredentialState>("command_code_set_api_key", { apiKey });
}

export function clearCommandCodeApiKey(): Promise<CommandCodeCredentialState> {
  return invoke<CommandCodeCredentialState>("command_code_clear_api_key");
}

/** 命令失败时 Rust 返回 `{ code }`；取不到时按未知处理。 */
export function commandErrorCode(error: unknown): string {
  if (error && typeof error === "object" && "code" in error) {
    const code = (error as { code: unknown }).code;
    if (typeof code === "string") return code;
  }
  return "unknown";
}
