import { describe, it, expect } from 'vitest';
import { readFileSync } from 'fs';
import { translations, type Language, type TranslationKey } from '../../i18n/translations';
import { API_SECTIONS, endpointAnchor } from '../docs-endpoints';
import { buildSiteIndex, searchSite, filterEndpoints, norm } from '../site-search';
import { isTypingTarget } from '../keyboard';

const tr = (lang: Language) => (k: TranslationKey) => translations[lang][k] ?? translations.zh[k] ?? k;
const zhIndex = buildSiteIndex(tr('zh'), 'zh');
const enIndex = buildSiteIndex(tr('en'), 'en');
const total = API_SECTIONS.reduce((n, s) => n + s.endpoints.length, 0);

describe('站内搜索索引', () => {
  it('覆盖全部 API 端点、官网页面和 AI 资源', () => {
    expect(total).toBeGreaterThan(30);
    expect(zhIndex.filter((e) => e.kind === 'endpoint')).toHaveLength(total);
    expect(zhIndex.filter((e) => e.kind === 'page').length).toBeGreaterThanOrEqual(10);
    expect(zhIndex.some((e) => e.href === '/llms.txt')).toBe(true);
  });

  it('条目 id 与端点锚点唯一(POST /mcp 的 5 个用途也互不冲突)', () => {
    expect(new Set(zhIndex.map((e) => e.id)).size).toBe(zhIndex.length);
    const anchors = API_SECTIONS.flatMap((s) => s.endpoints.map(endpointAnchor));
    expect(new Set(anchors).size).toBe(anchors.length);
    expect(endpointAnchor({ descKey: 'docs.section.memory.ep2.desc' })).toBe('ep-memory-ep2');
  });

  it('页面条目都指向 App.tsx 中真实存在的路由', () => {
    const app = readFileSync('src/App.tsx', 'utf-8');
    for (const e of zhIndex.filter((x) => x.kind === 'page')) {
      expect(app, e.href).toContain(`<Route path="${e.href.slice(1)}"`);
    }
  });

  it('端点结果深链到文档页对应锚点', () => {
    const hit = searchSite(zhIndex, '/v1/search')[0];
    expect(hit.title).toBe('POST /v1/search');
    expect(hit.href).toBe('#/docs?ep=ep-memory-ep2');
  });
});

describe('searchSite 排序与匹配', () => {
  it('路径段前缀优先:search → POST /v1/search 排第一', () => {
    expect(searchSite(enIndex, 'search')[0].title).toBe('POST /v1/search');
  });

  it('ticket → 申请 SSE 票据端点', () => {
    expect(searchSite(enIndex, 'ticket')[0].title).toBe('POST /v1/stream/ticket');
  });

  it('页面名精确命中排第一', () => {
    expect(searchSite(zhIndex, '文档')[0].href).toBe('#/docs');
    expect(searchSite(enIndex, 'benchmarks')[0].href).toBe('#/benchmarks');
  });

  it('多词 AND:delete memories 只返回删除记忆相关端点', () => {
    const r = searchSite(enIndex, 'delete memories');
    expect(r.length).toBeGreaterThan(0);
    expect(r[0].title).toBe('DELETE /v1/memories/:id');
    for (const e of r) expect(e.hay).toContain('delete');
  });

  it('跨语言关键词:英文界面也能用中文搜到页面', () => {
    expect(searchSite(enIndex, '主题')[0].title).toBe('Theme Center');
    expect(searchSite(zhIndex, 'theme')[0].title).toBe('主题中心');
  });

  it('中文描述可搜(zh 界面搜「身份」命中身份系统端点)', () => {
    const r = searchSite(zhIndex, '身份');
    expect(r.some((e) => e.title === 'POST /v1/identity/confirm')).toBe(true);
  });

  it('只命中隐藏关键词的页面排在真正描述该词的端点之后(记忆 → 端点优先于首页)', () => {
    const r = searchSite(zhIndex, '记忆');
    expect(r[0].kind).toBe('endpoint');
    expect(r.findIndex((e) => e.href === '#/')).toBeGreaterThan(0);
  });

  it('大小写与全角归一化', () => {
    expect(norm('ＳＥＡＲＣＨ')).toBe('search');
    expect(searchSite(enIndex, 'ＳＥＡＲＣＨ')[0].title).toBe('POST /v1/search');
  });

  it('空查询与无意义查询返回空;limit 生效', () => {
    expect(searchSite(enIndex, '   ')).toEqual([]);
    expect(searchSite(enIndex, 'zzqqxx-not-a-thing')).toEqual([]);
    expect(searchSite(enIndex, 'v1', 5)).toHaveLength(5);
  });
});

describe('filterEndpoints(文档页筛选)', () => {
  it('空查询保留全部', () => {
    const f = filterEndpoints(API_SECTIONS, '', tr('zh'));
    expect(f.reduce((n, s) => n + s.endpoints.length, 0)).toBe(total);
  });
  it('按方法 + 关键词筛选,分组顺序不变', () => {
    const f = filterEndpoints(API_SECTIONS, 'DELETE', tr('en'));
    expect(f.map((s) => s.section.titleKey)).toEqual(API_SECTIONS.map((s) => s.titleKey));
    const eps = f.flatMap((s) => s.endpoints);
    expect(eps.length).toBeGreaterThan(0);
    for (const ep of eps) expect(`${ep.method} ${ep.path} ${tr('en')(ep.descKey as TranslationKey)}`.toLowerCase()).toContain('delete');
  });
  it('分组标题也可匹配(runtime → L0 意志通道分组)', () => {
    const eps = filterEndpoints(API_SECTIONS, 'runtime', tr('en')).flatMap((s) => s.endpoints);
    expect(eps.map((e) => e.path)).toContain('/v1/runtime/heartbeat');
  });
});

describe('杂项', () => {
  it('isTypingTarget', () => {
    expect(isTypingTarget({ tagName: 'input' })).toBe(true);
    expect(isTypingTarget({ tagName: 'TEXTAREA' })).toBe(true);
    expect(isTypingTarget({ tagName: 'DIV', isContentEditable: true })).toBe(true);
    expect(isTypingTarget({ tagName: 'BUTTON' })).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
  });
  it('首屏 SiteSearch 入口不静态依赖搜索索引(索引随对话框懒加载)', () => {
    const entry = readFileSync('src/components/SiteSearch.tsx', 'utf-8');
    expect(entry).not.toMatch(/from '@\/lib\/(site-search|docs-endpoints)'/);
    expect(entry).toContain("lazy(() => import('./SiteSearchDialog'))");
  });
  it('Docs 页使用共享端点数据,且不再用重复的 method+path 作 key', () => {
    const docs = readFileSync('src/pages/Docs.tsx', 'utf-8');
    expect(docs).toContain("from '@/lib/docs-endpoints'");
    expect(docs).not.toContain('const API_SECTIONS');
    expect(docs).not.toContain('key={ep.method + ep.path}');
  });
});
