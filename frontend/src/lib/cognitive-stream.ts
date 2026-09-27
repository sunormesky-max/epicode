export interface EventStreamSource {
  onopen: ((event: Event) => void) | null;
  onmessage: ((event: MessageEvent) => void) | null;
  onerror: ((event: Event) => void) | null;
  close(): void;
}

interface TicketedStreamOptions {
  requestTicket: () => Promise<string>;
  canConnect: () => boolean;
  onMessage: (event: MessageEvent) => void;
  onConnectionChange?: (connected: boolean) => void;
  onError?: (error: unknown) => void;
  endpoint?: string;
  retryDelayMs?: number;
  maxRetryDelayMs?: number;
  createEventSource?: (url: string) => EventStreamSource;
}

export function createTicketedEventStream(options: TicketedStreamOptions) {
  const endpoint = options.endpoint ?? '/api/v1/stream';
  const initialRetryDelay = options.retryDelayMs ?? 1000;
  const maxRetryDelay = options.maxRetryDelayMs ?? 30000;
  const createEventSource = options.createEventSource ?? ((url) => new EventSource(url));
  let retryDelay = initialRetryDelay;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;
  let source: EventStreamSource | null = null;
  let generation = 0;
  let connectingGeneration: number | null = null;
  let paused = true;
  let stopped = false;

  function clearRetryTimer(): void {
    if (retryTimer) clearTimeout(retryTimer);
    retryTimer = null;
  }

  function closeSource(): void {
    if (!source) return;
    const current = source;
    source = null;
    current.onopen = null;
    current.onmessage = null;
    current.onerror = null;
    current.close();
  }

  function scheduleRetry(expectedGeneration: number): void {
    if (paused || stopped || generation !== expectedGeneration || retryTimer || !options.canConnect()) return;
    const delay = retryDelay;
    retryDelay = Math.min(maxRetryDelay, retryDelay * 2);
    retryTimer = setTimeout(() => {
      retryTimer = null;
      void connect();
    }, delay);
  }

  async function connect(): Promise<void> {
    if (paused || stopped || source || connectingGeneration === generation || !options.canConnect()) return;
    const currentGeneration = generation;
    connectingGeneration = currentGeneration;

    try {
      const ticket = await options.requestTicket();
      if (!ticket) throw new Error('Stream ticket request returned an empty ticket.');
      if (paused || stopped || generation !== currentGeneration || !options.canConnect()) return;

      const separator = endpoint.includes('?') ? '&' : '?';
      const current = createEventSource(`${endpoint}${separator}ticket=${encodeURIComponent(ticket)}`);
      source = current;
      current.onopen = () => {
        if (source !== current || generation !== currentGeneration) return;
        retryDelay = initialRetryDelay;
        options.onConnectionChange?.(true);
      };
      current.onmessage = options.onMessage;
      current.onerror = () => {
        if (source !== current || generation !== currentGeneration) return;
        closeSource();
        options.onConnectionChange?.(false);
        scheduleRetry(currentGeneration);
      };
    } catch (error) {
      if (!paused && !stopped && generation === currentGeneration) {
        options.onConnectionChange?.(false);
        options.onError?.(error);
        scheduleRetry(currentGeneration);
      }
    } finally {
      if (connectingGeneration === currentGeneration) connectingGeneration = null;
    }
  }

  function pause(): void {
    paused = true;
    generation++;
    connectingGeneration = null;
    clearRetryTimer();
    closeSource();
    options.onConnectionChange?.(false);
  }

  function resume(): void {
    if (stopped || !paused) return;
    paused = false;
    void connect();
  }

  function stop(): void {
    if (stopped) return;
    stopped = true;
    pause();
  }

  return { pause, resume, stop };
}
