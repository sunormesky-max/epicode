import { afterEach, describe, expect, it, vi } from 'vitest';
import { createTicketedEventStream, type EventStreamSource } from '../cognitive-stream';

class FakeEventSource implements EventStreamSource {
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  closed = false;
  readonly url: string;

  constructor(url: string) {
    this.url = url;
  }

  close(): void {
    this.closed = true;
  }

  fail(): void {
    this.onerror?.(new Event('error'));
  }
}

afterEach(() => vi.useRealTimers());

describe('ticketed cognitive stream', () => {
  it('requests a fresh one-use ticket after a disconnect and never puts an API key in the URL', async () => {
    vi.useFakeTimers();
    const sources: FakeEventSource[] = [];
    const requestTicket = vi.fn()
      .mockResolvedValueOnce('ticket-one')
      .mockResolvedValueOnce('ticket-two');
    const stream = createTicketedEventStream({
      requestTicket,
      canConnect: () => true,
      onMessage: () => undefined,
      retryDelayMs: 100,
      createEventSource: (url) => {
        const source = new FakeEventSource(url);
        sources.push(source);
        return source;
      },
    });

    stream.resume();
    await Promise.resolve();
    expect(sources[0].url).toBe('/api/v1/stream?ticket=ticket-one');
    expect(sources[0].url).not.toContain('tm-');

    sources[0].fail();
    expect(sources[0].closed).toBe(true);
    await vi.advanceTimersByTimeAsync(100);
    await Promise.resolve();

    expect(requestTicket).toHaveBeenCalledTimes(2);
    expect(sources[1].url).toBe('/api/v1/stream?ticket=ticket-two');
    stream.stop();
    expect(sources[1].closed).toBe(true);
  });

  it('ignores a ticket that resolves after the stream is paused for an account change', async () => {
    let resolveTicket: ((ticket: string) => void) | undefined;
    const sources: FakeEventSource[] = [];
    const stream = createTicketedEventStream({
      requestTicket: () => new Promise((resolve) => { resolveTicket = resolve; }),
      canConnect: () => true,
      onMessage: () => undefined,
      createEventSource: (url) => {
        const source = new FakeEventSource(url);
        sources.push(source);
        return source;
      },
    });

    stream.resume();
    stream.pause();
    resolveTicket?.('stale-ticket');
    await Promise.resolve();

    expect(sources).toHaveLength(0);
  });
});
