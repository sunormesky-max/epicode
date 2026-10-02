import { describe, expect, it } from 'vitest';
import { energyPercent, mergeEvents, sampleFreshness } from '../observation';

describe('observation presentation', () => {
  it('does not present missing or stale samples as live', () => {
    expect(sampleFreshness(0, 100_000)).toBe('waiting');
    expect(sampleFreshness(NaN, 100_000)).toBe('waiting');
    expect(sampleFreshness(40_000, 100_000)).toBe('fresh');
    expect(sampleFreshness(39_999, 100_000)).toBe('stale');
    expect(sampleFreshness(100_001, 100_000)).toBe('fresh');
  });
  it('updates repeated signals without duplicate rows and bounds retained history', () => {
    expect(mergeEvents([{ id: 2, text: 'updated' }, { id: 2, text: 'duplicate' }], [
      { id: 2, text: 'old' }, { id: 1, text: 'previous' }, { id: 0, text: 'expired' },
    ], 2)).toEqual([{ id: 2, text: 'updated' }, { id: 1, text: 'previous' }]);
  });
  it('clamps malformed energy readings for progress presentation', () => {
    expect([NaN, -100, 0, 5000, 12000].map(energyPercent)).toEqual([0, 0, 0, 50, 100]);
  });
});
