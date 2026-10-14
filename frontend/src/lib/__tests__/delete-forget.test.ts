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
const { deleteMemory, forgetMemory, invalidateCache, setAuth } = await import('../api');
const { translations } = await import('../../i18n/translations');

const ok = (data: unknown) => ({ ok: true, status: 200, text: async () => JSON.stringify({ protocol: { ok: true }, data }), json: async () => ({ protocol: { ok: true }, data }) });

describe('删除 vs 遗忘 语义', () => {
  beforeEach(() => { fetchMock.mockReset(); invalidateCache(); setAuth(null, 'u1'); });

  it('deleteMemory = DELETE /v1/memories/:id, mode purge', async () => {
    fetchMock.mockResolvedValue(ok({ status: 'deleted', id: 7, mode: 'purge', relations_removed: 2 }));
    const r = await deleteMemory(7);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toMatch(/\/v1\/memories\/7$/);
    expect(init.method).toBe('DELETE');
    expect(r.mode).toBe('purge');
  });

  it('forgetMemory = POST /v1/memories/:id/forget(可恢复)', async () => {
    fetchMock.mockResolvedValue(ok({ id: 7, valid_to: 1 }));
    await forgetMemory(7);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toMatch(/\/v1\/memories\/7\/forget$/);
    expect(init.method).toBe('POST');
  });

  it('删除确认文案在中英文都说明不可恢复并指向遗忘', () => {
    expect(translations.zh['dash.mem.deleteConfirm']).toMatch(/无法恢复/);
    expect(translations.zh['dash.mem.deleteConfirm']).toMatch(/遗忘/);
    expect(translations.en['dash.mem.deleteConfirm']).toMatch(/cannot be recovered/);
    expect(translations.en['dash.mem.deleteConfirm']).toMatch(/Forget/);
  });
});
