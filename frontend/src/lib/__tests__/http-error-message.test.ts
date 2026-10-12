import { beforeEach, describe, it, expect, vi } from 'vitest';

const store: Record<string, string> = {};
vi.stubGlobal('localStorage', {
  getItem: (k: string) => store[k] ?? null,
  setItem: (k: string, v: string) => { store[k] = v; },
  removeItem: (k: string) => { delete store[k]; },
  clear: () => { for (const k of Object.keys(store)) delete store[k]; },
});
const fetchMock = vi.fn();
vi.stubGlobal('fetch', fetchMock);
const { friendlyHttpError, request, invalidateCache, setAuth } = await import('../api');

const MSG = { 502: '网关错误', 503: '系统维护中' };
const resp = (status: number, body: string) => ({ ok: false, status, text: async () => body, json: async () => JSON.parse(body) });

describe('friendlyHttpError', () => {
  it('空响应体用状态码文案', () => { expect(friendlyHttpError(503, '', MSG)).toBe('系统维护中'); });
  it('网关 HTML 错误页不直接展示', () => {
    expect(friendlyHttpError(502, '<html><head><title>502 Bad Gateway</title></head><body><center><h1>502</h1></center></body></html>', MSG)).toBe('网关错误');
    expect(friendlyHttpError(502, '<!DOCTYPE html><html></html>', MSG)).toBe('网关错误');
  });
  it('简短纯文本保留原文', () => { expect(friendlyHttpError(400, 'title too long', MSG)).toBe('title too long'); });
  it('未知 5xx / 4xx 兜底', () => {
    expect(friendlyHttpError(500, '', {})).toBe('服务暂时不可用，请稍后重试。');
    expect(friendlyHttpError(418, '', {})).toBe('请求失败 (418)');
  });
});

describe('request() 错误体处理', () => {
  beforeEach(() => { fetchMock.mockReset(); invalidateCache(); setAuth(null, 'u1'); vi.useFakeTimers({ shouldAdvanceTime: true }); });

  it('5xx 空响应体不再抛出 "Unexpected end of JSON input"', async () => {
    fetchMock.mockResolvedValue(resp(503, ''));
    const p = request('/v1/timeline', { method: 'POST' }).catch((e: Error) => e.message);
    expect(await p).toBe('系统维护中，请稍后再试。');
  });
  it('JSON 业务错误仍原样透出', async () => {
    fetchMock.mockResolvedValue(resp(400, JSON.stringify({ error: 'content required' })));
    expect(await request('/v1/x', { method: 'POST' }).catch((e: Error) => e.message)).toBe('content required');
  });
  it('identity_not_confirmed 友好提示不变', async () => {
    fetchMock.mockResolvedValue(resp(403, JSON.stringify({ error: 'identity_not_confirmed' })));
    expect(await request('/v1/x', { method: 'POST' }).catch((e: Error) => e.message)).toContain('身份确认');
  });
});
