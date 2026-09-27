import { afterEach, describe, it, expect, beforeEach, vi } from 'vitest';

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
  logout, errMsg, invalidateCache, request, loginUser,
} = await import('../api');

afterEach(() => vi.restoreAllMocks());

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

  it('换账号时删除旧账号密钥并拒绝身份不匹配的密钥', () => {
    setAuth('tm-account-a', 'user-a');
    setAuth(null, 'user-b');
    expect(getApiKey()).toBeNull();
    expect(localStorageMock.getItem('epicode_api_key')).toBeNull();
    expect(localStorageMock.getItem('epicode_api_key_user_id')).toBeNull();

    setAuth('tm-account-b', 'user-b');
    localStorageMock.setItem('epicode_api_key_user_id', 'user-a');
    expect(getApiKey()).toBeNull();
  });

  it('不会在没有所有者标记时直接发送旧的未分区密钥', () => {
    localStorageMock.setItem('epicode_api_key', 'tm-legacy');
    localStorageMock.setItem('epicode_user_id', 'user-b');

    expect(getApiKey()).toBeNull();
    expect(localStorageMock.getItem('epicode_api_key_user_id')).toBeNull();
  });

  it('旧的未分区密钥不会随当前账号的 cookie 请求发送', async () => {
    localStorageMock.setItem('epicode_api_key', 'tm-account-a');
    localStorageMock.setItem('epicode_user_id', 'user-b');
    fetchMock.mockResolvedValueOnce(okJson({ memories_used: 0 }));

    await request('/v1/stats', { skipCache: true });

    const [, init] = fetchMock.mock.calls[0];
    expect((init as RequestInit).headers).not.toHaveProperty('X-API-Key');
    expect((init as RequestInit).credentials).toBe('include');
  });

  it('same-user legacy-key migration is kept only after server identity verification', async () => {
    localStorageMock.setItem('epicode_api_key', 'tm-legacy');
    localStorageMock.setItem('epicode_user_id', 'user-a');
    clearAuth({ preserveLegacyApiKey: true });
    expect(getUserId()).toBeNull();
    expect(getApiKey()).toBeNull();
    expect(localStorageMock.getItem('epicode_api_key')).toBe('tm-legacy');

    fetchMock
      .mockResolvedValueOnce(okJson({ success: true, user_id: 'user-a', plan: 'Free' }))
      .mockResolvedValueOnce(okJson({
        success: true,
        user_id: 'user-a',
        request_key_matches: true,
        masked_key: 'tm-le...acy',
      }));

    await loginUser('user-a', 'password');

    const [verifyUrl, verifyInit] = fetchMock.mock.calls[1];
    expect(String(verifyUrl)).toContain('/v1/api-key');
    expect((verifyInit as RequestInit).headers).toHaveProperty('X-API-Key', 'tm-legacy');
    expect(getApiKey()).toBe('tm-legacy');
  });

  it('does not migrate an unowned key when the server identifies a different account', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    localStorageMock.setItem('epicode_api_key', 'tm-account-a');
    localStorageMock.setItem('epicode_user_id', 'user-b');
    fetchMock
      .mockResolvedValueOnce(okJson({ success: true, user_id: 'user-b', plan: 'Free' }))
      .mockResolvedValueOnce(okJson({ success: false, error: 'conflicting authentication credentials' }, 401));

    await loginUser('user-b', 'password');

    expect(getUserId()).toBe('user-b');
    expect(getApiKey()).toBeNull();
    expect(localStorageMock.getItem('epicode_api_key')).toBeNull();
  });

  it('登录另一个账号前先移除旧密钥，不会与新会话 cookie 一起发送', async () => {
    setAuth('tm-account-a', 'user-a');
    fetchMock.mockResolvedValueOnce(okJson({ success: true, user_id: 'user-b', plan: 'Free' }));

    await loginUser('user-b', 'password');

    const [loginUrl, loginInit] = fetchMock.mock.calls[0];
    expect(String(loginUrl)).toContain('/v1/login');
    expect((loginInit as RequestInit).headers).not.toHaveProperty('X-API-Key');
    expect(getUserId()).toBe('user-b');
    expect(getApiKey()).toBeNull();

    fetchMock.mockResolvedValueOnce(okJson({ memories_used: 0 }));
    await request('/v1/stats', { skipCache: true });
    const [, statsInit] = fetchMock.mock.calls[1];
    expect((statsInit as RequestInit).headers).not.toHaveProperty('X-API-Key');
    expect((statsInit as RequestInit).credentials).toBe('include');
  });

  it('账号切换会中止已发出的旧账号请求', async () => {
    setAuth('tm-account-a', 'user-a');
    let oldSignal: AbortSignal | undefined;
    fetchMock.mockImplementationOnce((_url: string, init: RequestInit) => new Promise<Response>((_, reject) => {
      oldSignal = init.signal ?? undefined;
      oldSignal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), { once: true });
    }));
    const pendingRequest = request('/v1/stats').catch((error: unknown) => error);
    await Promise.resolve();
    expect(oldSignal).toBeDefined();

    fetchMock.mockResolvedValueOnce(okJson({ success: true, user_id: 'user-b', plan: 'Free' }));
    await loginUser('user-b', 'password');

    expect(oldSignal?.aborted).toBe(true);
    await expect(pendingRequest).resolves.toMatchObject({
      message: expect.stringMatching(/Authentication changed/),
    });
  });

  it('同一账号仍可迁移其旧密钥', async () => {
    setAuth('tm-account-a', 'user-a');
    fetchMock.mockResolvedValueOnce(okJson({ success: true, user_id: 'user-a', plan: 'Free' }));

    await loginUser('user-a', 'password');

    expect(getApiKey()).toBe('tm-account-a');
    fetchMock.mockResolvedValueOnce(okJson({ memories_used: 0 }));
    await request('/v1/stats', { skipCache: true });
    const [, init] = fetchMock.mock.calls[1];
    expect((init as RequestInit).headers).toHaveProperty('X-API-Key', 'tm-account-a');
  });

  it('clearAuth清key+userId+历史聊天(同机换号防泄漏)', () => {
    setAuth('tm-abc', 'user-a');
    localStorageMock.setItem('epicode_chat_history_user-a', '[{"role":"u"}]');
    localStorageMock.setItem('epicode_chat_history', '旧格式残留');
    clearAuth();
    expect(getApiKey()).toBeNull();
    expect(getUserId()).toBeNull();
    expect(localStorageMock.getItem('epicode_api_key_user_id')).toBeNull();
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
