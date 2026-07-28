# ADR-0013：macOS 读取 Claude Code 钥匙串凭据

- 状态：已确认
- 日期：2026-07-27
- 相关文档：[额度领域模型](../额度领域模型.md) 第 5 节、[技术架构](../技术架构.md)「凭据与权限」、[ADR-0006](ADR-0006-首版只支持Codex-OAuth凭据.md)

## 背景

[额度领域模型](../额度领域模型.md) 第 5.1 节原有决策是「不读取 macOS Keychain，Claude Code 只读 `~/.claude/.credentials.json`」，理由是保持两个平台行为一致、避免首版引入平台专有安全存储读取。同一节已经把「凭据只在 Keychain、本地没有 `.credentials.json` 的用户会被误判为 `no_credentials`」标注为待确认风险。

2026-07-27 进入第 12 阶段前在产品所有者的 macOS 主机上核对凭据来源（只检查存在性，不读取内容）：

| 来源 | 结果 |
|---|---|
| `~/.codex/auth.json` | 存在 |
| `CODEX_HOME` | 未设置 |
| `~/.claude/.credentials.json` | **不存在** |
| `~/.claude.json` | 存在 |
| Keychain generic password `Claude Code-credentials` | **存在** |

风险不再是「待确认」：按原决策，Claude Code 在唯一可用的开发与验证主机上恒为 `no_credentials`，第 12 阶段的 Claude 闭环无法实现，也无法验证故障隔离。

## 决策

macOS 上的 Claude Code 凭据发现顺序改为：

1. `~/.claude/.credentials.json`；
2. 文件缺失、为空或无法解析时，读取 macOS Keychain 的 generic password，service 为 `Claude Code-credentials`，account 为当前登录用户；
3. 显示用邮箱仍可由 `~/.claude.json` 的 `oauthAccount.emailAddress` 兜底。

Windows 维持只读 `~/.claude/.credentials.json`，不引入 Windows Credential Manager 读取。两个平台的凭据来源因此**不再一致**，这是本决策明确接受的代价。

Keychain 访问使用 `security-framework` crate 直接调用系统 API，不启动 `security` 子进程——[ADR-0007](ADR-0007-首版不实现CLI兜底与delegated-refresh.md) 拒绝 CLI 兜底的理由（启动外部进程、PTY、看门狗与首版权限克制原则冲突）同样适用于凭据读取路径。

Keychain 项属于 Claude Code 自己的存储，CC Trace 只读它、按 [ADR-0014](ADR-0014-token刷新结果回写外部凭据.md) 在刷新成功时回写同一项，不创建自己的 Keychain 项，也不触碰 `com.nanvon.cctrace.credentials` 这个预留 namespace。

## 理由

- 不读 Keychain 会让 Claude Code 在实际存在有效登录态的机器上显示「未检测到凭据」，这是错误信息，不是克制。
- Claude Code CLI 在 macOS 上把凭据放进 Keychain 是常见形态，不是边缘情况；把它当作不支持等于放弃一半 Provider。
- 平台一致性本身不是目标，它服务于「同一份代码在两个平台行为可预期」。凭据存放位置由外部工具决定，CC Trace 只能适配，不能靠拒绝读取来制造一致性。
- 只增加一条读取来源，不改变契约、状态语义与调度；`no_credentials` 与 `unsupported` 的判定规则不变。

## 替代方案

| 方案 | 不采用的原因 |
|---|---|
| 维持只读文件 | Claude 侧在唯一验证主机上无法闭环，第 12 阶段只能完成一半 |
| 首版整体不做 Claude Code | 双 Provider 与故障隔离是 [产品范围](../产品范围.md) 的首版必须项 |
| 通过 `security` CLI 子进程读取 | 与 ADR-0007 的权限克制原则冲突，且错误处理依赖解析 CLI 输出 |
| Windows 同步引入 Credential Manager 读取 | 没有证据表明 Claude Code 在 Windows 上使用它；无验证主机，属于凭空实现 |

## 后果

- `docs/额度领域模型.md` 第 5 节凭据来源表与 5.1 决策表需要改写，原「待确认（风险）」条目转为已解决。
- Rust 侧新增 `credentials` 模块与 macOS 条件编译分支；Windows 分支只保留文件来源。
- 首次读取 Keychain 会触发一次系统钥匙串访问授权弹窗。这是 macOS 的既有行为，需要写进首次启动说明与实机走查清单。
- 已授予的钥匙串访问在 designated requirement 变化时失效并重新弹窗。DR 由签名标识和证书 CN 共同决定，两者都不变时重新编译、重新签名都不影响，见下方 2026-07-28 实机记录；换用不同证书或改 `identifier` 则会失效。

### 2026-07-27 实机记录

首次实机运行确认每次重新编译后都会重新要求授权。原因是 `cargo build` 产出的是 adhoc 签名（`Signature=adhoc`、`flags=0x20002(adhoc,linker-signed)`），没有稳定的签名标识，钥匙串 ACL 只能按 CDHash 记录被授权的程序，而 CDHash 随二进制内容变化。

Swift 版 cc-bar 不弹窗，是因为它通过 `/usr/bin/security` 子进程读取：写进 ACL 的是 Apple 签名、标识恒定的系统工具，不是 cc-bar 自己。代价是该授权对**所有**调用 `security` 的程序生效，边界比按应用授权弱。这一点不改变本 ADR 的选择。

解决方式是用固定证书签名，使 ACL 记录 designated requirement 而非 CDHash。按 [ADR-0016](ADR-0016-不购买Apple开发者账号.md) 使用免费手段取得的证书即可，不需要 Apple 开发者账号。

### 2026-07-28 实机记录：开发期签名已验证有效

在 macOS 上实测，上一条的推断成立：固定证书签名后，钥匙串授权一次长期有效，重新编译不再弹窗。

实现是 `src-tauri/.cargo/config.toml` 的 cargo runner `scripts/dev-sign.sh`，在每次 `cargo run` 启动二进制前重新签名，流程见 [工程与发布](../工程与发布.md) 第 1.1 节。

两个实测发现改变了原计划：

**用 Apple Development 证书，不用自签名证书。** 产品所有者机器上已有免费 Apple ID 通过 Xcode 取得的 Apple Development 证书（有效期至 2027-05-27），不涉及付费账号，与 ADR-0016 不冲突。它比自签名更好，因为签出的 designated requirement 是：

```
identifier "com.nanvon.cctrace" and anchor apple generic
  and certificate leaf[subject.CN] = "Apple Development: <账号> (<ID>)"
  and certificate 1[field.1.2.840.113635.100.6.2.1]
```

证书按 **subject.CN** 匹配而非指纹。证书到期后 Xcode 换发的同名证书 CN 不变，ACL 仍然匹配，不需要重新授权。自签名证书没有 anchor，DR 会退化成绑定证书指纹，换证书即失效。

**签名标识必须显式指定。** cargo 默认写入的是 `cc_trace-<metadata hash>`，该 hash 由构建配置决定，改 feature、profile 或依赖都可能让它变化，而它是 DR 的组成部分。脚本从 `tauri.conf.json` 读 `identifier` 并用 `codesign --identifier` 钉死，dev 与打包产物的 DR 因此是同一个。

> 待确认：正式打包产物（`tauri build`）的 DR 是否与 dev 一致，尚未实测。若不一致，用户从 dev 切到正式版本时会被要求重新授权一次。
- 两平台凭据来源不一致，[测试策略](../测试策略.md) 与双平台验收必须分别记录 macOS 与 Windows 的凭据发现结果，不得互相推断。
- Keychain 读取失败（用户拒绝授权、项被删除）必须落在 `no_credentials` 或凭据类 `error`，不得降级成 `offline`。

## 复审条件

- 若实机发现 Claude Code 在 Windows 上同样使用系统凭据存储，重新评估 Windows 分支，而不是自动认为文件来源足够。
- 若钥匙串授权弹窗在正式签名分发后反复出现并影响可用性，重新评估是否需要引导用户改用文件凭据，而不是自动退回只读文件。
- 若后续 Claude Code 变更 Keychain 的 service 名称或 payload 结构，按 `unsupported` 处理并更新本决策，不得猜测新结构。
