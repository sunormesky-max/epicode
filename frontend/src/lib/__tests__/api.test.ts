import { describe, it, expect, beforeEach, vi } from 'vitest';

// Mock localStorage before importing the module under test.
const store: Record<string, string> = {};
const localStorageMock = {
  getItem: (key: string) => store[key] ?? null,
  setItem: (key: string, value: string) => {
    store[key] = value;
  },
  removeItem: (key: string) => {
    delete store[key];
  },
  clear: () => {
    for (const k of Object.keys(store)) delete store[k];
  },
};
vi.stubGlobal('localStorage', localStorageMock);

// Mock fetch so we never hit the network.
const fetchMock = vi.fn();
vi.stubGlobal('fetch', fetchMock);

// Import after mocks are in place.
const {
  getApiKey,
  getUserId,
  setAuth,
  clearAuth,
  isAuthenticated,
  invalidateCache,
  clearCache,
  request,
  logout,
} = await import('../api');

describe('auth utilities', () => {
  beforeEach(() => {
    localStorageMock.clear();
    fetchMock.mockReset();
  });

  it('setAuth stores only userId (cookie carries the session)', () => {
    setAuth('user-456');
    expect(getUserId()).toBe('user-456');
    // api key must NOT be persisted (审计 2026-09 高优 #4)
    expect(getApiKey()).toBeNull();
  });

  it('setAuth purges legacy plaintext key left by older versions', () => {
    localStorageMock.setItem('epicode_api_key', 'legacy-key');
    setAuth('user-1');
    expect(getApiKey()).toBeNull();
  });

  it('clearAuth removes userId and legacy key', () => {
    setAuth('user-456');
    clearAuth();
    expect(getApiKey()).toBeNull();
    expect(getUserId()).toBeNull();
  });

  it('isAuthenticated returns false before setAuth', () => {
    expect(isAuthenticated()).toBe(false);
  });

  it('isAuthenticated returns true after setAuth', () => {
    setAuth('user-456');
    expect(isAuthenticated()).toBe(true);
  });
});

describe('cache isolation between accounts (审计 2026-09 高优 #3)', () => {
  beforeEach(() => {
    localStorageMock.clear();
    fetchMock.mockReset();
    clearCache();
  });

  const okJson = (v: unknown) => ({
    ok: true,
    status: 200,
    json: async () => v,
    text: async () => '',
  });

  it('user B must not see user A cached response after switching', async () => {
    // 用户 A 登录并请求
    setAuth('user-A');
    fetchMock.mockResolvedValueOnce(okJson({ secret: 'A-data' }));
    const a1 = await request<{ secret: string }>('/v1/stats');
    expect(a1.secret).toBe('A-data');

    // 同一 endpoint, A 的缓存命中(不再发起请求)
    const a2 = await request<{ secret: string }>('/v1/stats');
    expect(a2.secret).toBe('A-data');
    expect(fetchMock).toHaveBeenCalledTimes(1);

    // 换号: setAuth 会清缓存; B 的请求必须重新走网络
    setAuth('user-B');
    fetchMock.mockResolvedValueOnce(okJson({ secret: 'B-data' }));
    const b1 = await request<{ secret: string }>('/v1/stats');
    expect(b1.secret).toBe('B-data');
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('clearAuth drops the cache', async () => {
    setAuth('user-A');
    fetchMock.mockResolvedValueOnce(okJson({ v: 1 }));
    await request('/v1/timeline');
    clearAuth();
    setAuth('user-B');
    fetchMock.mockResolvedValueOnce(okJson({ v: 2 }));
    const r = await request<{ v: number }>('/v1/timeline');
    expect(r.v).toBe(2);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});

describe('cache invalidation', () => {
  beforeEach(() => {
    localStorageMock.clear();
    fetchMock.mockReset();
    clearCache();
  });

  const okJson = (v: unknown) => ({
    ok: true,
    status: 200,
    json: async () => v,
    text: async () => '',
  });

  it('invalidateCache really drops the entry (uid-prefixed key, 审计二轮回归)', async () => {
    setAuth('user-A');
    // 写入缓存
    fetchMock.mockResolvedValueOnce(okJson({ value: 1 }));
    await request<{ value: number }>('/v1/stats');
    // 失效后必须重新走网络(而非命中旧缓存)
    invalidateCache('/v1/stats');
    fetchMock.mockResolvedValueOnce(okJson({ value: 2 }));
    const r = await request<{ value: number }>('/v1/stats');
    expect(r.value).toBe(2);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('invalidateCache only drops matching prefixes', async () => {
    setAuth('user-A');
    fetchMock.mockResolvedValueOnce(okJson({ v: 'stats' }));
    await request('/v1/stats');
    invalidateCache('/v1/timeline');
    // /v1/stats 未失效: 仍命中缓存, 不发新请求
    const r = await request<{ v: string }>('/v1/stats');
    expect(r.v).toBe('stats');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

describe('logout closure (审计二轮: 必须服务端失效 cookie)', () => {
  beforeEach(() => {
    localStorageMock.clear();
    fetchMock.mockReset();
    clearCache();
  });

  it('logout calls backend /v1/logout then clears local state', async () => {
    setAuth('user-A');
    fetchMock.mockResolvedValueOnce({
      ok: true,
      status: 200,
      json: async () => ({ success: true }),
      text: async () => '',
    });
    await logout();
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toContain('/v1/logout');
    expect((init as RequestInit).method).toBe('POST');
    expect(getUserId()).toBeNull();
    expect(getApiKey()).toBeNull();
  });

  it('logout still clears local state when backend unreachable', async () => {
    setAuth('user-A');
    fetchMock.mockRejectedValueOnce(new Error('network down'));
    await logout();
    expect(getUserId()).toBeNull();
  });
});
