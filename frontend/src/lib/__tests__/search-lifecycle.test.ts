import { afterEach, describe, expect, it, vi } from 'vitest';
import { SearchLifecycle, visibleSearchResults } from '../search-lifecycle';

afterEach(() => vi.useRealTimers());

describe('memory search lifecycle', () => {
  it('keeps a successful zero-result search separate from the timeline', () => {
    expect(visibleSearchResults(true, 'no matching memory', [])).toEqual([]);
    expect(visibleSearchResults(false, '', [])).toBeNull();
  });

  it('aborts in-flight work and clears a pending search when canceled', async () => {
    vi.useFakeTimers();
    const lifecycle = new SearchLifecycle();
    const request = lifecycle.start();
    const pendingDebounce = vi.fn();

    lifecycle.schedule(pendingDebounce, 500);
    lifecycle.cancel();
    await vi.advanceTimersByTimeAsync(500);

    expect(request.signal.aborted).toBe(true);
    expect(pendingDebounce).not.toHaveBeenCalled();
  });
});
