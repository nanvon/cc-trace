//! 阶段 0 性能基线：B1/B2/B5/B8 场景测量（docs/性能与功耗优化方案.md 阶段 0）。
//!
//! 实现见 `usage_bench_impl.rs`。基准工具依赖 `perf-baseline` feature（统计计数器与
//! 基准扫描入口），不带该 feature 时本 example 编译为空程序，不影响 CI 的
//! `cargo clippy --all-targets` 与 `cargo test`。
//!
//! 用法：
//!   cargo run --release --example usage_bench --features perf-baseline --
//!     --dataset-dir /tmp/cc-trace-datasets/M --scenario B1 --work-dir /tmp/cc-trace-bench

#[cfg(feature = "perf-baseline")]
include!("usage_bench/impl.rs");

#[cfg(not(feature = "perf-baseline"))]
fn main() {
    eprintln!("usage_bench requires the perf-baseline feature");
}
