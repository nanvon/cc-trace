//! macOS 钥匙串访问：读取并回写 Claude Code 自己的凭据项。
//!
//! 决策与边界见
//! [ADR-0013](../../../docs/决策/ADR-0013-macOS读取ClaudeCode钥匙串凭据.md) 与
//! [ADR-0014](../../../docs/决策/ADR-0014-token刷新结果回写外部凭据.md)：
//!
//! - 只访问 Claude Code 自己的 generic password 项，不创建 CC Trace 自有条目。
//! - 使用系统 API，不启动 `security` 子进程——[ADR-0007](../../../docs/决策/ADR-0007-首版不实现CLI兜底与delegated-refresh.md)
//!   拒绝外部进程的理由同样适用于凭据路径。
//! - 首次读取会触发一次系统授权弹窗；用户拒绝时落 [`KeychainRead::Unreadable`]，
//!   由调用方映射成凭据类 `error`，绝不能说成「没有凭据」。
//!
//! 非 macOS 平台没有这个来源，所有函数返回「不可用」，凭据发现退回文件来源。

use std::io;

use crate::providers::credentials::Secret;

/// Claude Code 写入钥匙串时使用的 service 名。它属于 Claude Code，不属于 CC Trace。
#[cfg(target_os = "macos")]
const CLAUDE_SERVICE: &str = "Claude Code-credentials";

/// 一次钥匙串读取的结局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeychainRead {
    Found {
        payload: Secret,
        account: Secret,
    },
    /// 没有这一项，或本平台没有这个来源。
    Missing,
    /// 项存在但读不出来：授权被拒、钥匙串已锁定、系统调用失败。
    Unreadable,
}

#[cfg(target_os = "macos")]
pub fn read_claude_credentials() -> KeychainRead {
    read_claude_credentials_matching(None)
}

#[cfg(target_os = "macos")]
pub fn read_claude_credentials_for_account(account: &Secret) -> KeychainRead {
    read_claude_credentials_matching(Some(account))
}

#[cfg(target_os = "macos")]
fn read_claude_credentials_matching(account: Option<&Secret>) -> KeychainRead {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};

    /// `errSecItemNotFound`：没有匹配项，等价于「没登录过」。
    const ITEM_NOT_FOUND: i32 = -25_300;

    let mut search = ItemSearchOptions::new();
    search
        .class(ItemClass::generic_password())
        .service(CLAUDE_SERVICE)
        .load_attributes(true)
        .load_data(true)
        .limit(Limit::Max(1));
    if let Some(account) = account {
        search.account(account.expose());
    }

    let items = match search.search() {
        Ok(items) => items,
        Err(error) if error.code() == ITEM_NOT_FOUND => return KeychainRead::Missing,
        // 授权被拒或钥匙串锁定都落这里。不降级成 Missing：用户有登录态，只是我们拿不到。
        Err(_) => return KeychainRead::Unreadable,
    };

    let Some(attributes) = items.first().and_then(|item| item.simplify_dict()) else {
        return KeychainRead::Missing;
    };

    let Some(payload) = attributes
        .get("v_Data")
        .filter(|payload| !payload.trim().is_empty())
    else {
        return KeychainRead::Missing;
    };
    let Some(account) = attributes
        .get("acct")
        .filter(|account| !account.trim().is_empty())
    else {
        // 没有 account 就无法保证刷新结果写回刚才读到的同一项，按不可读失败关闭。
        return KeychainRead::Unreadable;
    };

    KeychainRead::Found {
        payload: Secret::new(payload),
        account: Secret::new(account),
    }
}

#[cfg(target_os = "macos")]
pub fn write_claude_credentials(
    account: &Secret,
    expected_payload: &Secret,
    payload: &Secret,
) -> io::Result<()> {
    use security_framework::os::macos::passwords::find_generic_password;

    let (current, mut item) = find_generic_password(None, CLAUDE_SERVICE, account.expose())
        .map_err(|_| io::Error::other("existing keychain item is no longer writable"))?;
    if current.as_ref() != expected_payload.expose().as_bytes() {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "claude keychain item changed during token refresh",
        ));
    }
    item.set_password(payload.expose().as_bytes())
        .map_err(|_| io::Error::other("keychain write failed"))
}

#[cfg(not(target_os = "macos"))]
pub fn read_claude_credentials() -> KeychainRead {
    // Windows 上 Claude Code 是否使用系统凭据存储没有验证过，不凭空实现，见 ADR-0013。
    KeychainRead::Missing
}

#[cfg(not(target_os = "macos"))]
pub fn read_claude_credentials_for_account(_account: &Secret) -> KeychainRead {
    KeychainRead::Missing
}

#[cfg(not(target_os = "macos"))]
pub fn write_claude_credentials(
    _account: &Secret,
    _expected_payload: &Secret,
    _payload: &Secret,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this platform has no keychain credential source",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_result_never_prints_the_payload() {
        let read = KeychainRead::Found {
            payload: Secret::new("{\"claudeAiOauth\":{}}"),
            account: Secret::new("private-account"),
        };
        assert!(!format!("{read:?}").contains("claudeAiOauth"));
        assert!(!format!("{read:?}").contains("private-account"));
    }

    /// 非 macOS 上钥匙串来源不存在，凭据发现必须退回文件来源而不是报错。
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn platforms_without_a_keychain_report_the_source_as_absent() {
        assert_eq!(read_claude_credentials(), KeychainRead::Missing);
        assert!(matches!(
            read_claude_credentials_for_account(&Secret::new("account")),
            KeychainRead::Missing
        ));
        assert!(
            write_claude_credentials(
                &Secret::new("account"),
                &Secret::new("{}"),
                &Secret::new("{}")
            )
            .is_err()
        );
    }
}
