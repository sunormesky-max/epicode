import { beforeEach, describe, it, expect, vi } from 'vitest';

// 并发 GET 合并(single-flight)回归测试
const store: Record<string, string> = {};
vi.stubGlobal('localStorage', {
  getItem: (k: string) => store[k] ?? null,
  setItem: (k: string, v: string) => { store[k] = v; },
  removeItem: (k: string) => { delete store[k]; },
  clear: () => { for (const k of Object.keys(store)) delete store[k]; },
});
const fetchMock = vi.fn();
vi.stubGlobal('fetch', fetchMock);

const { request, invalidateCache, setAuth, inflightRequestCount } = await import('../api');

function deferredResponse(v: unknown) {
  let resolve!: () => void;
  const p = new Promise<Response>((r) => {
    resolve = () => r({ ok: true, status: 200, json: async () => v, text: async () => JSON.stringify(v) } as Response);
  });
  return { p, resolve };
}

describe('request(): 并发 GET 合并', () => {
  beforeEach(() => { fetchMock.mockReset(); invalidateCache(); setAuth(null, 'user-a'); });

  it('同一 GET 并发两次只发一次网络请求,两个调用方拿到同一结果', async () => {
    const d = deferredResponse({ n: 1 });
    fetchMock.mockReturnValueOnce(d.p);
    const a = request<{ n: number }>('/v1/stats');
    const b = request<{ n: number }>('/v1/stats');
    expect(inflightRequestCount()).toBe(1);
    d.resolve();
    expect(await a).toEqual({ n: 1 });
    expect(await b).toEqual({ n: 1 });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(inflightRequestCount()).toBe(0);
  });

  it('不同路径 / 不同 body / POST 不合并', async () => {
    fetchMock.mockImplementation(async () => ({ ok: true, status: 200, json: async () => ({}), text: async () => '{}' }));
    await Promise.all([request('/v1/stats'), request('/v1/timeline'), request('/v1/x', { method: 'POST', body: {} }), request('/v1/x', { method: 'POST', body: {} })]);
    expect(fetchMock).toHaveBeenCalledTimes(4);
  });

  it('skipCache 的 GET 不合并(调用方明确要新数据)', async () => {
    fetchMock.mockImplementation(async () => ({ ok: true, status: 200, json: async () => ({}), text: async () => '{}' }));
    await Promise.all([request('/v1/stats', { skipCache: true }), request('/v1/stats', { skipCache: true })]);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('一个调用方取消只影响它自己,另一个仍拿到结果,底层 fetch 未被取消', async () => {
    const d = deferredResponse({ ok: 1 });
    let fetchSignal: AbortSignal | undefined;
    fetchMock.mockImplementationOnce((_u: string, init: RequestInit) => { fetchSignal = init.signal ?? undefined; return d.p; });
    const c = new AbortController();
    const a = request('/v1/stats', { signal: c.signal });
    const b = request('/v1/stats');
    c.abort();
    await expect(a).rejects.toThrow();
    expect(fetchSignal?.aborted).toBe(false);
    d.resolve();
    expect(await b).toEqual({ ok: 1 });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('所有调用方都取消时真正 abort 底层 fetch(保持卸载即取消),之后可重新发起', async () => {
    let fetchSignal: AbortSignal | undefined;
    fetchMock.mockImplementationOnce((_u: string, init: RequestInit) => {
      fetchSignal = init.signal ?? undefined;
      return new Promise((_r, rej) => fetchSignal!.addEventListener('abort', () => rej(Object.assign(new Error('aborted'), { name: 'AbortError' }))));
    });
    const c1 = new AbortController(), c2 = new AbortController();
    const a = request('/v1/stats', { signal: c1.signal });
    const b = request('/v1/stats', { signal: c2.signal });
    c1.abort();
    expect(fetchSignal?.aborted).toBe(false);
    c2.abort();
    expect(fetchSignal?.aborted).toBe(true);
    await expect(a).rejects.toThrow();
    await expect(b).rejects.toThrow();
    expect(inflightRequestCount()).toBe(0);
    fetchMock.mockResolvedValueOnce({ ok: true, status: 200, json: async () => ({ again: 1 }), text: async () => '' });
    expect(await request('/v1/stats')).toEqual({ again: 1 });
  });

  it('错误同样共享,并在结束后清理', async () => {
    fetchMock.mockResolvedValue({ ok: false, status: 400, json: async () => ({}), text: async () => JSON.stringify({ error: 'bad thing' }) });
    const a = request('/v1/stats'); const b = request('/v1/stats');
    await expect(a).rejects.toThrow('bad thing');
    await expect(b).rejects.toThrow('bad thing');
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(inflightRequestCount()).toBe(0);
  });

  it('不同用户的同一路径不合并', async () => {
    const d = deferredResponse({ u: 'a' });
    fetchMock.mockReturnValueOnce(d.p);
    const a = request('/v1/stats');
    setAuth(null, 'user-b');
    fetchMock.mockResolvedValueOnce({ ok: true, status: 200, json: async () => ({ u: 'b' }), text: async () => '' });
    const b = request('/v1/stats');
    d.resolve();
    await a.catch(() => {});
    expect(await b).toEqual({ u: 'b' });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});
