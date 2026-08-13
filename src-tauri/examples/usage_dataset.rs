//! 阶段 0 性能基线：从脱敏 Fixture 机械派生 M/L/R 数据集。
//!
//! 见 docs/性能与功耗优化方案.md 阶段 0：M/L 只机械复制并改写脱敏 ID 与时间，
//! 不引入真实标题、路径、账号或消息正文；输出目录不入仓库。实现见
//! `usage_dataset_impl.rs`，不带 `perf-baseline` feature 时本 example 编译为空程序。
//!
//! 用法：
//!   cargo run --release --example usage_dataset -- --kind M --out /tmp/cc-trace-datasets/M

#[cfg(feature = "perf-baseline")]
include!("usage_dataset/impl.rs");

#[cfg(not(feature = "perf-baseline"))]
fn main() {
    eprintln!("usage_dataset requires the perf-baseline feature");
}
