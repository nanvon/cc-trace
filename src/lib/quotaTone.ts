/**
 * 额度余量 → 视觉基调的**唯一映射**。
 *
 * 这是与 `src/lib/status.ts` 并列的第二个着色维度，分档由
 * [ADR-0017](../../docs/决策/ADR-0017-系统区域显示额度数字与余量分档.md) 拥有：
 *
 * - 余量基调（本模块）只驱动百分比读数与进度条填充。
 * - 状态基调（`status.ts` 的 `StatusTone`）只驱动状态提示条、状态点与状态词。
 *
 * 两者必须并存：「凭据失效」和「只剩 3%」是两件不同的事，任何一个维度都无法
 * 代替另一个。组件不得自己再判断一遍百分比区间。
 */

import type { RailTreatment } from "./status";

/** 余量基调。`none` 表示没有可展示的数值，不是「余量为零」。 */
export type QuotaTone = "none" | "ok" | "warning" | "low" | "danger";

/** 分档边界。改这里必须同时改 ADR-0017 的分档表。 */
const LOW_THRESHOLD = 20;
const WARNING_THRESHOLD = 50;

/**
 * 剩余百分比 → 余量基调。
 *
 * `remainingPercent` 已由 Rust clamp 到 0–100，本函数不再重算范围。
 */
export function quotaTone(remainingPercent: number | null): QuotaTone {
  if (remainingPercent === null) {
    return "none";
  }
  if (remainingPercent <= 0) {
    return "danger";
  }
  if (remainingPercent < LOW_THRESHOLD) {
    return "low";
  }
  if (remainingPercent <= WARNING_THRESHOLD) {
    return "warning";
  }
  return "ok";
}

/**
 * 参与展示的余量基调。
 *
 * 快照不新鲜时整体降级为中性：旧数据不得用鲜艳颜色宣称当前额度紧张。
 * `loading` 与 `empty` 同样没有可信数值，一并降级。
 */
export function displayQuotaTone(
  remainingPercent: number | null,
  treatment: RailTreatment,
): QuotaTone {
  if (treatment !== "filled") {
    return "none";
  }
  return quotaTone(remainingPercent);
}
