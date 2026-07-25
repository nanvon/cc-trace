//! Provider 凭据发现、额度请求、响应解析和标准化。
//!
//! 第 11 阶段只有 [`synthetic`] 这一个实现，产出合成快照用于验证桌面壳与状态矩阵。
//! 第 12 阶段接入真实 Codex / Claude Code 时**只替换本模块内部的实现**：
//! [`QuotaProvider`] trait、[`ProviderFetchOutcome`] 与 `crate::contracts` 保持不变，
//! 调度、命令与前端都不需要改动。

pub mod synthetic;

use std::future::Future;
use std::pin::Pin;

use chrono::Duration;

use crate::contracts::{ErrorKind, ProviderId, ProviderIdentity, QuotaSnapshot};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 一次额度请求的结局。
///
/// 每个分支对应 `docs/状态与错误模型.md` 第 4 节的一类失败来源；调度层据此映射到
/// `ProviderAvailability`，不允许出现兜底的「未知错误」。
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderFetchOutcome {
    Success {
        identity: Option<ProviderIdentity>,
        snapshot: QuotaSnapshot,
    },
    /// 凭据文件缺失或无可用凭据，不发起请求。
    NoCredentials,
    /// 凭据存在但形式不受首版支持，不发起请求。
    Unsupported,
    /// 无网络、DNS 失败、连接被拒、超时或传输中断。
    Offline,
    /// HTTP 429。`retry_after` 来自服务端 `Retry-After`，缺失时由退避阶梯决定。
    RateLimited { retry_after: Option<Duration> },
    /// 凭据类（401/403/刷新失败）或协议类（无法解析、字段不兼容）失败。
    Failed { kind: ErrorKind },
}

/// 一个 Provider 的额度来源。
pub trait QuotaProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    /// 获取一次额度。实现方负责凭据发现、请求、解析与标准化，
    /// 并保证不把凭据、端点原文或响应原文带进返回值。
    fn fetch(&self) -> BoxFuture<'_, ProviderFetchOutcome>;
}
