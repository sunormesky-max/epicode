export type Freshness = 'waiting' | 'fresh' | 'stale';

// Freshness describes the last sample, not transport connectivity or engine health.
export function sampleFreshness(timestamp: number, now: number): Freshness {
  if (!Number.isFinite(timestamp) || timestamp <= 0) return 'waiting';
  return now - timestamp > 60_000 ? 'stale' : 'fresh';
}

export function mergeEvents<T extends { id: number }>(incoming: T[], previous: T[], limit = 20): T[] {
  const seen = new Set<number>();
  return [...incoming, ...previous].filter(event => {
    if (seen.has(event.id)) return false;
    seen.add(event.id);
    return true;
  }).slice(0, limit);
}

export function energyPercent(value: number): number {
  return Number.isFinite(value) ? Math.max(0, Math.min(100, value / 100)) : 0;
}
