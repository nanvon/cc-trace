//! CC Trace 自己的秘密存储。
//!
//! 与 `providers::credentials` 的区别：那里读的是**别人的**凭据来源（外部 CLI 的
//! 登录态，只读或按 [ADR-0014] 回写 token），这里存的是**我们自己**的数据——导入的
//! Codex 账号凭据与用户手动填写的 Command Code API Key。它们没有外部真源，所以可写可删。
//!
//! 平台实现见 [ADR-0035]：macOS 用系统钥匙串，Windows 用凭据管理器。两个平台都是
//! 「当前用户可见、无需管理员」的系统级存储，不落到应用数据目录的明文文件里。
//!
//! 这里的所有返回值都不含明文；明文只在 [`Secret`] 里，它的 `Debug` 永远是占位符。
//!
//! [ADR-0014]: ../../../../docs/决策/ADR-0014-token刷新结果回写外部凭据.md
//! [ADR-0035]: ../../../../docs/决策/ADR-0035-服务矩阵与Windows凭据存储.md

use std::io;

use crate::providers::credentials::Secret;

/// 我们的条目在所有系统凭据里的 service / target 前缀。
/// 用应用标识做前缀，用户能在系统凭据界面一眼认出来源，也不会撞上别人的条目。
const SERVICE: &str = "cc-trace";

/// 导入 Codex 账号凭据的槽位名。
pub fn codex_account_slot(index: u32) -> String {
    format!("codex-imported-{index}")
}

/// 手动填写的 Command Code API Key 槽位名。
pub const COMMAND_CODE_API_KEY_SLOT: &str = "command-code-api-key";

/// 一次秘密读取的结局。与 [`crate::providers::credentials::Discovery`] 保持同构：
/// 「没有」与「有但拿不到」必须分开，后者是凭据类 `error` 而不是 `no_credentials`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretRead {
    Found(Secret),
    Missing,
    /// 系统拒绝了访问（钥匙串授权被拒、凭据管理器不可用）。
    Denied,
    /// 其他失败：IO、编码、参数非法。
    Failed,
}

/// 写入或删除的结局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretWrite {
    Ok,
    Denied,
    Failed,
}

impl SecretWrite {
    pub fn is_ok(self) -> bool {
        self == Self::Ok
    }
}

#[cfg(target_os = "macos")]
fn target_name(slot: &str) -> String {
    format!("{SERVICE}/{slot}")
}

#[cfg(target_os = "macos")]
pub fn read(slot: &str) -> SecretRead {
    use security_framework::os::macos::passwords::find_generic_password;

    /// `errSecItemNotFound`：没有这一项。
    const ITEM_NOT_FOUND: i32 = -25_300;
    /// `errSecAuthFailed` / `errSecUserCanceled` / `errSecInteractionNotAllowed`。
    const DENIED: [i32; 3] = [-25_293, -128, -25_308];

    match find_generic_password(None, SERVICE, &target_name(slot)) {
        Ok((payload, _item)) => SecretRead::Found(Secret::new(String::from_utf8_lossy(&payload))),
        Err(error) if error.code() == ITEM_NOT_FOUND => SecretRead::Missing,
        Err(error) if DENIED.contains(&error.code()) => SecretRead::Denied,
        Err(_) => SecretRead::Failed,
    }
}

#[cfg(target_os = "macos")]
pub fn write(slot: &str, value: &Secret) -> SecretWrite {
    use security_framework::os::macos::keychain::SecKeychain;

    /// `errSecInteractionNotAllowed`：钥匙串已锁定且不允许弹窗。
    const INTERACTION_NOT_ALLOWED: i32 = -25_308;
    /// `errSecAuthFailed`：授权被拒。
    const AUTH_FAILED: i32 = -25_293;

    // `SecKeychain::set_generic_password` 内部先查找再更新，不会留下重复项。
    let result = SecKeychain::default().and_then(|keychain| {
        keychain.set_generic_password(SERVICE, &target_name(slot), value.expose().as_bytes())
    });
    match result {
        Ok(()) => SecretWrite::Ok,
        Err(error) if [INTERACTION_NOT_ALLOWED, AUTH_FAILED].contains(&error.code()) => {
            SecretWrite::Denied
        }
        Err(_) => SecretWrite::Failed,
    }
}

#[cfg(target_os = "macos")]
pub fn delete(slot: &str) -> SecretWrite {
    use security_framework::os::macos::passwords::find_generic_password;

    /// `errSecItemNotFound`：本来就不存在，按删除成功处理。
    const ITEM_NOT_FOUND: i32 = -25_300;

    match find_generic_password(None, SERVICE, &target_name(slot)) {
        Ok((_payload, item)) => {
            item.delete();
            SecretWrite::Ok
        }
        Err(error) if error.code() == ITEM_NOT_FOUND => SecretWrite::Ok,
        Err(_) => SecretWrite::Failed,
    }
}

/// Windows：凭据管理器（`CredReadW` / `CredWriteW` / `CredDeleteW`）。
///
/// 常量与结构体来自 `windows-sys` 的官方绑定，不手抄数值：`CRED_TYPE_GENERIC`、
/// `CRED_PERSIST_LOCAL_MACHINE`、`CREDENTIALW` 的字段顺序都由绑定保证。
/// 条目按当前用户存 `CRED_PERSIST_LOCAL_MACHINE`，重启后仍然存在；目标名是
/// `cc-trace/<slot>`，用户在「凭据管理器 → Windows 凭据」里能看到并自行删除。
#[cfg(windows)]
pub fn read(slot: &str) -> SecretRead {
    use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
    use windows_sys::Win32::Security::Credentials::{CRED_TYPE_GENERIC, CredFree, CredReadW};

    let target = to_wide(&format!("{SERVICE}/{}", slot));
    let mut credential: *mut windows_sys::Win32::Security::Credentials::CREDENTIALW =
        std::ptr::null_mut();

    // SAFETY: `target` 是以 NUL 结尾的 UTF-16 缓冲；`credential` 是有效输出指针，
    // 成功时必须由 `CredFree` 释放。
    let ok = unsafe {
        CredReadW(
            target.as_ptr(),
            CRED_TYPE_GENERIC,
            0,
            &mut credential as *mut _,
        )
    };
    if ok == 0 {
        let error = unsafe { GetLastError() };
        return if error == ERROR_NOT_FOUND {
            SecretRead::Missing
        } else if error == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED {
            SecretRead::Denied
        } else {
            SecretRead::Failed
        };
    }
    if credential.is_null() {
        return SecretRead::Failed;
    }

    // SAFETY: `CredReadW` 成功时返回一个已初始化的 `CREDENTIALW`。
    let blob = unsafe {
        let credential = &*credential;
        if credential.CredentialBlobSize == 0 || credential.CredentialBlob.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
            .to_vec()
        }
    };
    unsafe { CredFree(credential.cast()) };

    let text = decode_blob(&blob);
    if text.is_empty() {
        SecretRead::Missing
    } else {
        SecretRead::Found(Secret::new(text))
    }
}

#[cfg(windows)]
pub fn write(slot: &str, value: &Secret) -> SecretWrite {
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, GetLastError};
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
    };

    let target = to_wide(&format!("{SERVICE}/{}", slot));
    let user_name = to_wide(SERVICE);
    let mut blob = encode_blob(value.expose());
    let blob_size = u32::try_from(blob.len()).unwrap_or(0);
    if blob_size == 0 {
        return SecretWrite::Failed;
    }

    let credential = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr() as *mut u16,
        Comment: std::ptr::null_mut(),
        LastWritten: Default::default(),
        CredentialBlobSize: blob_size,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: std::ptr::null_mut(),
        UserName: user_name.as_ptr() as *mut u16,
    };

    // SAFETY: 各指针指向在调用期间有效的缓冲，且 `Type` 与 blob 编码一致。
    let ok = unsafe { CredWriteW(&credential, 0) };
    if ok != 0 {
        return SecretWrite::Ok;
    }
    let error = unsafe { GetLastError() };
    if error == ERROR_ACCESS_DENIED {
        SecretWrite::Denied
    } else {
        SecretWrite::Failed
    }
}

#[cfg(windows)]
pub fn delete(slot: &str) -> SecretWrite {
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_NOT_FOUND, GetLastError};
    use windows_sys::Win32::Security::Credentials::{CRED_TYPE_GENERIC, CredDeleteW};

    let target = to_wide(&format!("{SERVICE}/{}", slot));
    // SAFETY: `target` 是以 NUL 结尾的 UTF-16 缓冲。
    let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if ok != 0 {
        return SecretWrite::Ok;
    }
    let error = unsafe { GetLastError() };
    if error == ERROR_NOT_FOUND {
        // 已经不存在就是删除成功。
        SecretWrite::Ok
    } else if error == ERROR_ACCESS_DENIED {
        SecretWrite::Denied
    } else {
        SecretWrite::Failed
    }
}

/// 非 macOS 与 Windows 的平台没有定义好的系统秘密存储：明确失败，不退回明文文件。
#[cfg(not(any(target_os = "macos", windows)))]
pub fn read(_slot: &str) -> SecretRead {
    SecretRead::Denied
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn write(_slot: &str, _value: &Secret) -> SecretWrite {
    SecretWrite::Denied
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn delete(_slot: &str) -> SecretWrite {
    SecretWrite::Denied
}

/// 把文本编码成系统凭据的 blob。
///
/// Windows 凭据管理器对 generic credential 的约定是 UTF-16LE 字符串；写成 UTF-8 也能
/// 自洽，但在系统凭据界面里会显示成乱码，所以按平台约定来。
#[cfg(windows)]
fn encode_blob(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<u8>>()
}

#[cfg(windows)]
fn decode_blob(blob: &[u8]) -> String {
    let units: Vec<u16> = blob
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_owned()
}

#[cfg(windows)]
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 供调用方把结局映射成应用错误时使用：只有系统明确拒绝才算「被拒」，其余算一般失败。
pub fn failure_kind(result: SecretWrite) -> io::ErrorKind {
    match result {
        SecretWrite::Denied => io::ErrorKind::PermissionDenied,
        _ => io::ErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_stable_and_namespaced() {
        assert_eq!(codex_account_slot(0), "codex-imported-0");
        assert_eq!(codex_account_slot(7), "codex-imported-7");
        assert_eq!(COMMAND_CODE_API_KEY_SLOT, "command-code-api-key");
    }

    #[test]
    fn a_secret_never_prints_its_payload() {
        let read = SecretRead::Found(Secret::new("sk-live-abcdef"));
        assert!(!format!("{read:?}").contains("sk-live"));
    }

    #[cfg(windows)]
    #[test]
    fn the_blob_round_trips_as_utf16() {
        let encoded = encode_blob("{\"key\":\"值\"}");
        assert_eq!(decode_blob(&encoded), "{\"key\":\"值\"}");
    }
}
