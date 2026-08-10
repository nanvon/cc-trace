/**
 * 额度历史（Timeline）的归组与主账号去重逻辑。
 *
 * `quota_events` 只记录整数剩余值变化点；同一条序列由
 * (provider, identity_key, window_kind, window_id) 唯一确定。身份指纹变化或窗口切换都会
 * 产生新序列。主账号镜像去重的落地：界面只展示每个 Provider 的**活动序列**——包含该
 * Provider 最新事件点的序列，旧身份/旧窗口的序列不再展示，避免同一账号的历史出现多条。
 */
import type { QuotaHistoryEvent } from "../usage/contracts";
import type { ProviderId, QuotaWindowKind } from "./contracts";

export interface QuotaSeries {
  provider: ProviderId;
  identityKey: string;
  windowKind: QuotaWindowKind;
  windowId: string | null;
  /** 时间升序的事件点。 */
  points: QuotaHistoryEvent[];
}

export function seriesKey(event: QuotaHistoryEvent): string {
  return [event.provider, event.identityKey, event.windowKind, event.windowId ?? ""].join("|");
}

/** 按 (provider, identity_key, window_kind, window_id) 归组，序列内时间升序。 */
export function groupSeries(events: QuotaHistoryEvent[]): QuotaSeries[] {
  const groups = new Map<string, QuotaSeries>();
  for (const event of events) {
    const key = seriesKey(event);
    let series = groups.get(key);
    if (!series) {
      series = {
        provider: event.provider,
        identityKey: event.identityKey,
        windowKind: event.windowKind,
        windowId: event.windowId,
        points: [],
      };
      groups.set(key, series);
    }
    series.points.push(event);
  }
  const series = [...groups.values()];
  for (const group of series) {
    group.points.sort(
      (left, right) =>
        left.observedAt.localeCompare(right.observedAt) ||
        left.remainingPercent - right.remainingPercent,
    );
  }
  return series;
}

/** 每个 Provider 的活动序列：包含该 Provider 最新事件点的序列。 */
export function activeSeriesByProvider(events: QuotaHistoryEvent[]): Map<ProviderId, QuotaSeries> {
  const series = groupSeries(events);
  const latestIndex = new Map<ProviderId, { at: string; index: number }>();
  series.forEach((candidate, index) => {
    const latest = candidate.points[candidate.points.length - 1];
    const current = latestIndex.get(candidate.provider);
    if (!current || latest.observedAt > current.at) {
      latestIndex.set(candidate.provider, { at: latest.observedAt, index });
    }
  });

  const active = new Map<ProviderId, QuotaSeries>();
  for (const [provider, { index }] of latestIndex) {
    active.set(provider, series[index]);
  }
  return active;
}

export function latestEvent(series: QuotaSeries): QuotaHistoryEvent {
  return series.points[series.points.length - 1];
}

/** 表格行模型：事件点与其相对前一点的整数变化；第一点（无前值）为 `null`。 */
export interface QuotaEventRow {
  event: QuotaHistoryEvent;
  deltaPercent: number | null;
}

/** 序列内每个点相对前一点的整数变化，与 cc-bar `QuotaChangeEvent.deltaPercent` 同口径。 */
export function eventRows(series: QuotaSeries): QuotaEventRow[] {
  return series.points.map((point, index) => ({
    event: point,
    deltaPercent:
      index === 0 ? null : point.remainingPercent - series.points[index - 1].remainingPercent,
  }));
}

/** 今日 delta：当日最早的序列点与最新点的差值；当天没有点返回 `null`。 */
export function todayDelta(series: QuotaSeries, now: Date): number | null {
  const startOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const todayPoints = series.points.filter((point) => new Date(point.observedAt) >= startOfDay);
  if (todayPoints.length === 0) return null;
  const first = todayPoints[0].remainingPercent;
  const last = todayPoints[todayPoints.length - 1].remainingPercent;
  return last - first;
}
