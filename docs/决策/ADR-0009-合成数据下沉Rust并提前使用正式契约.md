# ADR-0009：桌面壳合成数据下沉到 Rust，并提前使用正式三维契约

- 状态：已确认
- 日期：2026-07-25
- 相关文档：[技术架构](../技术架构.md) 核心 contracts、[状态与错误模型](../状态与错误模型.md)、[桌面壳验证记录](../桌面壳验证记录.md)、[ADR-0005](ADR-0005-状态使用三维模型.md)

## 背景

第 11 阶段的第一版桌面壳把合成额度数据放在 Vue 里（`src/features/quota/preview.ts`），并用一个额外事件 `shell://refresh-preview` 驱动刷新动画。同时 `src/features/quota/contracts.ts` 里写了一个把三个维度压成单一 `SnapshotFreshness` 的简化枚举——而它实际上一行也没有被用到，界面用的是只有三个字符串字段的 `PreviewProvider`。

结果是三个问题叠在一起：

1. 刷新状态有两个来源（`shell://refresh-preview` 与将来的 `quota://refresh-state`），违反[状态与错误模型](../状态与错误模型.md)第 7 节「不为状态引入第二套并行来源」。
2. 文档描述与实现不符：文档说预览期「把三维压成单一 `SnapshotFreshness`」，实现连这个简化模型都没在用。
3. 第 12 阶段接入真实 Provider 时，前端的数据形状、事件、store 和组件都要重写一遍，桌面壳阶段验证过的交互等于白验证。

## 决策

- 三维契约（`RefreshState`、`SnapshotFreshness`、`ProviderAvailability`）在 Rust 侧定义为**最终形态**，第 12 阶段不再改动。
- 合成数据下沉到 `src-tauri/src/providers/synthetic.rs`，实现与真实 Provider 相同的 `QuotaProvider` trait，产出相同的 `ProviderFetchOutcome`。
- 刷新编排、请求合并、节流与退避在桌面壳阶段就用真实实现（`scheduler/`），只是数据源是合成的。
- 删除 `shell://refresh-preview`。刷新状态的唯一来源是 `quota://refresh-state`。
- debug 构建提供 `dev_set_scenario` 切换 9 个验证场景；release 构建通过 `#[cfg(debug_assertions)]` 完全编译掉。

## 理由

- 状态语义是这个产品最容易做错的部分。让它在有合成数据、可随时切换 9 种场景的环境下先跑通，比在真实网络和真实凭据的噪声里调试便宜得多。
- 第 12 阶段的工作因此收敛成一件事：**替换 `providers/` 模块内部的实现**。trait、契约、调度、命令与整个前端都不动。
- 退避与状态转换是纯逻辑，现在就能写单元测试，不需要等真实 Provider。[状态与错误模型](../状态与错误模型.md)第 2 节的组合矩阵已逐行覆盖。
- 合成数据经过与真实数据完全相同的路径，因此它不构成第二套状态源——这正是原方案的问题所在。

## 替代方案

| 方案 | 不采用的原因 |
|---|---|
| 保持合成数据在前端，只把 TS 类型补全 | 第二状态源仍在；第 12 阶段前端要重写一遍 |
| 桌面壳阶段完全不做状态模型，等 Provider 接入再一起做 | 9 种状态的视觉与交互无法在第 11 阶段验证，等于把风险全部推到第 12 阶段 |
| 用真实 Provider 但指向 mock server | 需要凭据发现与网络栈，属第 12 阶段范围，且无法离线走查 |

## 后果

- 桌面壳阶段的 Rust 代码量明显增加（契约、调度、退避、场景），但这些代码在第 12 阶段全部保留。
- `providers/synthetic.rs` 整份文件在第 12 阶段被真实实现替换或移除，`dev_set_scenario` 与前端场景切换器一并删除。
- 界面上必须持续、明确地标注数据是合成的（`preview.*` 文案命名空间），避免任何人把走查结果当成真实额度。

## 复审条件

- 第 12 阶段接入真实 Provider 时发现 `ProviderFetchOutcome` 无法表达某类真实失败——那说明[状态与错误模型](../状态与错误模型.md)第 4 节的失败分类需要先修订。
