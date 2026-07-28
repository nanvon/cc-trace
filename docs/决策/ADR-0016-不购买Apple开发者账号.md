# ADR-0016：不购买 Apple 开发者账号

- 状态：已确认
- 日期：2026-07-27
- 修订：2026-07-28，开发期签名由自签名证书改为免费 Apple ID 的 Apple Development 证书
- 修订：2026-07-28，GitHub 公开发布产物改用 ad-hoc 签名，不上传个人证书；「不购买开发者账号」与开发期固定签名两个结论不变
- 相关文档：[工程与发布](../工程与发布.md) 第 5 节、[ADR-0013](ADR-0013-macOS读取ClaudeCode钥匙串凭据.md)

## 背景

第 17 阶段原计划「配置 macOS 独立签名、公证和安装包」。公证（notarization）需要 Apple Developer Program 会员资格，费用为每年 99 美元。

2026-07-27 实机运行触发钥匙串授权弹窗时，产品所有者明确表示不会购买 Apple 开发者账号，且这一立场不因功能便利而改变。

2026-07-28 进一步确认仓库未来公开，并参考 Swift 版 cc-bar 的现有 GitHub Actions：
公开构建不上传个人证书，使用 ad-hoc 签名即可自动产出可手动放行的 macOS 包。此前把
Apple Development 证书同时用于开发与发布，需要把证书私钥导出到 GitHub，仍不能通过
Gatekeeper，也没有在其他人的机器上验证钥匙串 ACL，因此不再作为公开发布默认方案。

## 决策

CC Trace 不购买 Apple Developer Program 会员资格。由此确定：

- **不做公证。** macOS 产物不经 Apple 公证，Gatekeeper 会拦截首次打开，用户需要右键「打开」或在系统设置里放行。
- **开发期使用免费 Apple ID 的 Apple Development 证书。** 本 ADR 原定使用自签名证书；2026-07-28 实测后改为 Apple Development 证书，因为其 designated requirement 按 subject.CN 匹配，重新编译后钥匙串 ACL 仍然有效。理由与实测见 [ADR-0013](ADR-0013-macOS读取ClaudeCode钥匙串凭据.md)。
- **GitHub 公开发布产物使用 ad-hoc 签名。** workflow 显式设置 `APPLE_SIGNING_IDENTITY=-`，不读取、不上传或生成个人证书与私钥。Apple Silicon 与 Intel 分别构建 DMG。
- **接受版本更新后的钥匙串授权代价。** CC Trace 直接通过系统 API 读取 Claude Code 钥匙串；ad-hoc 产物的 CDHash 随版本变化，因此安装新版本后首次读取时可能需要再次选择「始终允许」。不为消除这次授权而退回 `/usr/bin/security` 子进程。
- **`0.x` Release 先作为测试发布。** 自动化只创建 Draft Release；完成双平台实机验收后再人工公开，不承诺陌生用户开箱即用。
- 发布说明必须写明 Gatekeeper 提示是预期行为，并给出打开方法。

Windows 代码签名同样不购买证书，产物会触发 SmartScreen 提示。该结论未经实机验证。

## 理由

- 这是产品所有者的既定立场，不是可以用技术方案绕过的约束。
- 公证只影响分发体验，不影响功能。CC Trace 的核心价值是本机额度可见，用户群体是会用 Codex 与 Claude Code CLI 的开发者，右键打开一次对他们不构成障碍。
- 开发期固定证书避免高频重新编译反复授权；公开发布频率低，可以接受每次版本更新后的一次授权，二者不需要使用同一签名策略。
- ad-hoc 发布不把个人签名私钥交给 GitHub，也不会把 Apple Development 误写成受 Gatekeeper 信任的公开分发签名。
- Swift 版 cc-bar 能在 ad-hoc 发布下保持钥匙串授权，是因为它通过 `/usr/bin/security` 读取；CC Trace 保留直接系统 API 的更窄授权边界，明确接受不同的更新体验。

## 替代方案

| 方案 | 不采用的原因 |
|---|---|
| 购买 Developer ID 并公证 | 产品所有者明确不付费 |
| GitHub 发布使用 Apple Development 证书 | 需要上传个人证书私钥，不解决 Gatekeeper；其他机器的运行与钥匙串 ACL 尚未验证 |
| 开发期也保持 ad-hoc | 每次重新编译都会让钥匙串 ACL 失效，严重影响日常开发 |
| 改用 `security` 子进程绕开钥匙串授权 | 授权落到系统工具而不是 CC Trace，削弱安全边界，且不解决 Gatekeeper |
| 自签名证书（本 ADR 原方案） | designated requirement 绑定证书指纹，换证书即失效；Apple Development 证书同样免费但按 CN 匹配，见 [ADR-0013](ADR-0013-macOS读取ClaudeCode钥匙串凭据.md) |
| 只发布 Windows 版本 | macOS 是产品所有者的主力平台 |

## 后果

- [工程与发布](../工程与发布.md) 第 5 节与发布检查单需要去掉公证相关要求，改为验证「Gatekeeper 提示与打开方法已写入发布说明」。
- 自动更新（若未来引入）在无公证的前提下需要重新评估可行性。
- 分发渠道不包括 Mac App Store——本来也不可能，`macos-private-api` 已经排除了这条路。
- 任何建议都不得以「购买开发者账号」或「购买代码签名证书」为前提；开发期固定签名使用免费 Apple ID 的 Apple Development 证书，GitHub 发布使用 ad-hoc 签名。
- GitHub Release workflow 不需要任何 Apple Secret；公开发布前必须实测两种 macOS 架构的安装、Gatekeeper 放行和版本更新后的钥匙串授权。
- Apple Development 证书有有效期（当前这张至 2027-05-27）。到期后由 Xcode 换发同名证书，CN 不变因此 ACL 继续有效；这条路径尚未实际走过，到期前需要复核。

## 复审条件

- 只有产品所有者主动改变立场时才重新评估，不因功能需求或分发摩擦自动重提。
