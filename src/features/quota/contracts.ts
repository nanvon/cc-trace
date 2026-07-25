export type ProviderId = "codex" | "claude";

export type SnapshotFreshness =
  | "loading"
  | "live"
  | "stale"
  | "offline"
  | "error";

export interface QuotaWindow {
  label: string;
  remainingPercent: number;
  resetsAt: string | null;
}

export interface ProviderSnapshot {
  provider: ProviderId;
  freshness: SnapshotFreshness;
  refreshedAt: string | null;
  windows: QuotaWindow[];
}
