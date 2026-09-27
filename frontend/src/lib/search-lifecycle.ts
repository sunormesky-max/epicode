export class SearchLifecycle {
  private debounceTimer: ReturnType<typeof setTimeout> | null = null;
  private controller: AbortController | null = null;

  schedule(action: () => void, delayMs: number): void {
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    this.debounceTimer = setTimeout(() => {
      this.debounceTimer = null;
      action();
    }, delayMs);
  }

  start(): AbortController {
    this.controller?.abort();
    this.controller = new AbortController();
    return this.controller;
  }

  cancel(): void {
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    this.debounceTimer = null;
    this.controller?.abort();
    this.controller = null;
  }
}

export function visibleSearchResults<T>(
  searchMode: boolean,
  query: string,
  results: T[],
): T[] | null {
  return searchMode && query.trim() ? results : null;
}
