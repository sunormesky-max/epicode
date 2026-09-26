import { describe, it, expect, beforeEach, vi } from 'vitest';

// ── api.ts 核心路径测试(第四批迭代): 此文件承载认证+缓存,
// 四轮审计中被改5次且每次靠手工验证 — 本套件让回归有网 ──

// localStorage mock: 存储键直接作为对象 own enumerable 属性(方法 non-enumerable)
// — Object.keys(localStorage) 返回存储键, 与真实行为一致(clearAuth依赖)
const localStorageMock: Record<string, string> & Storage = {} as never;
for (const [name, fn] of Object.entries({
  getItem: (key: string) => localStorageMock[key] ?? null,
  setItem: (key: string, value: string) => { localStorageMock[key] = value; },
  removeItem: (key: string) => { delete localStorageMock[key]; },
  clear: () => { for (const k of Object.keys(localStorageMock)) delete localStorageMock[k]; },
})) {
  Object.defineProperty(localStorageMock, name, { value: fn, enumerable: false });
}
vi.stubGlobal('localStorage', localStorageMock);
const fetchMock = vi.fn();
vi.stubGlobal('fetch', fetchMock);


const {
  getApiKey, getUserId, setAuth, clearAuth, isAuthenticated,
  logout, errMsg, invalidateCache, request,
} = await import('../api');

const okJson = (v: unknown, status = 200) => ({
  ok: status >= 200 && status < 300,
  status,
  json: async () => v,
  text: async () => '',
});

describe('认证状态', () => {
  beforeEach(() => { localStorageMock.clear(); fetchMock.mockReset(); invalidateCache(); });

  it('setAuth(null) 不落地key, 只存userId(key仅注册首显持有)', () => {
    setAuth(null, 'user-a');
    expect(getApiKey()).toBeNull();
    expect(getUserId()).toBe('user-a');
    expect(isAuthenticated()).toBe(true);
  });

  it('setAuth带key时落地(注册路径首次回显)', () => {
    setAuth('tm-abc', 'user-a');
    expect(getApiKey()).toBe('tm-abc');
  });

  it('clearAuth清key+userId+历史聊天(同机换号防泄漏)', () => {
    setAuth('tm-abc', 'user-a');
    localStorageMock.setItem('epicode_chat_history_user-a', '[{"role":"u"}]');
    localStorageMock.setItem('epicode_chat_history', '旧格式残留');
    clearAuth();
    expect(getApiKey()).toBeNull();
    expect(getUserId()).toBeNull();
    expect(localStorageMock.getItem('epicode_chat_history_user-a')).toBeNull();
    expect(localStorageMock.getItem('epicode_chat_history')).toBeNull();
  });

  it('logout先调后端再清本地', async () => {
    setAuth('tm-abc', 'user-a');
    fetchMock.mockResolvedValueOnce(okJson({ success: true }));
    await logout();
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain('/v1/logout');
    expect((init as RequestInit).method).toBe('POST');
    expect(getUserId()).toBeNull();
  });

  it('logout后端不可达仍清本地', async () => {
    setAuth('tm-abc', 'user-a');
    fetchMock.mockRejectedValueOnce(new Error('network'));
    await logout();
    expect(getUserId()).toBeNull();
  });
});

describe('缓存: 用户隔离与失效(历史双bug的永久回归网)', () => {
  beforeEach(() => { localStorageMock.clear(); fetchMock.mockReset(); invalidateCache(); });

  it('键含uid: 换号后不命中前用户缓存', async () => {
    setAuth(null, 'user-a');
    fetchMock.mockResolvedValueOnce(okJson({ secret: 'A' }));
    expect((await request<{ secret: string }>('/v1/stats')).secret).toBe('A');
    expect(fetchMock).toHaveBeenCalledTimes(1);

    setAuth(null, 'user-b');
    fetchMock.mockResolvedValueOnce(okJson({ secret: 'B' }));
    expect((await request<{ secret: string }>('/v1/stats')).secret).toBe('B');
    expect(fetchMock).toHaveBeenCalledTimes(2); // 未命中A的缓存
  });

  it('同用户30s内命中缓存(零额外请求)', async () => {
    setAuth(null, 'user-a');
    fetchMock.mockResolvedValueOnce(okJson({ v: 1 }));
    await request('/v1/timeline');
    await request('/v1/timeline');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('invalidateCache真失效(键用endpoint含/api前缀的历史bug回归网)', async () => {
    setAuth(null, 'user-a');
    fetchMock.mockResolvedValueOnce(okJson({ v: 1 }));
    await request('/v1/stats');
    invalidateCache('/v1/stats');
    fetchMock.mockResolvedValueOnce(okJson({ v: 2 }));
    const r = await request<{ v: number }>('/v1/stats');
    expect(r.v).toBe(2);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('invalidateCache无参清全量', async () => {
    setAuth(null, 'user-a');
    fetchMock.mockResolvedValueOnce(okJson({ v: 1 }));
    await request('/v1/stats');
    fetchMock.mockResolvedValueOnce(okJson({ v: 1 }));
    await request('/v1/timeline');
    invalidateCache();
    fetchMock.mockResolvedValueOnce(okJson({ v: 9 }));
    const r = await request<{ v: number }>('/v1/stats');
    expect(r.v).toBe(9);
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it('skipCache绕过缓存', async () => {
    setAuth(null, 'user-a');
    fetchMock.mockResolvedValueOnce(okJson({ v: 1 }));
    await request('/v1/stats');
    fetchMock.mockResolvedValueOnce(okJson({ v: 2 }));
    const r = await request<{ v: number }>('/v1/stats', { skipCache: true });
    expect(r.v).toBe(2);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('429抛限流错误信息', async () => {
    setAuth(null, 'user-a');
    fetchMock.mockResolvedValueOnce(okJson({ error: 'rate' }, 429));
    await expect(request('/v1/stats')).rejects.toThrow(/请求过于频繁|Rate limit/);
  });
});

describe('errMsg: 错误信息提取', () => {
  it('Error实例取message', () => expect(errMsg(new Error('boom'))).toBe('boom'));
  it('普通对象走String兜底(实现仅识别Error/string)', () => expect(errMsg({ error: 'E1' })).toBe('[object Object]'));
  it('JSON字符串原样返回(实现不做JSON解析)', () => expect(errMsg('{"error":"E2"}')).toBe('{"error":"E2"}'));
  it('兜底字符串', () => expect(errMsg('裸文本')).toBe('裸文本'));
});
