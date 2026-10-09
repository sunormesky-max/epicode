import { describe, it, expect } from 'vitest';
import { readFileSync } from 'fs';
import { ROUTE_META, resolvePageMeta, normalizePath, applyPageMeta, currentPageTitle, isConsolePath } from '../page-meta';

const appSrc = readFileSync('src/App.tsx', 'utf-8');
const routePaths = [...appSrc.matchAll(/<Route path="([^"]+)"/g)].map((m) => m[1]).filter((p) => p !== '*');

describe('page-meta 覆盖与质量', () => {
  it('App.tsx 中每个路由都有中英文元信息', () => {
    expect(routePaths.length).toBeGreaterThan(15);
    for (const p of routePaths) {
      expect(ROUTE_META[p], `缺少 ${p}`).toBeDefined();
      expect(ROUTE_META[p].zh.title.length, p).toBeGreaterThan(0);
      expect(ROUTE_META[p].en.title.length, p).toBeGreaterThan(0);
    }
  });

  it('没有多余的(已失效路由的)元信息', () => {
    for (const p of Object.keys(ROUTE_META)) expect(routePaths, p).toContain(p);
  });

  it('同一语言内标题互不重复', () => {
    for (const lang of ['zh', 'en'] as const) {
      const titles = routePaths.map((p) => resolvePageMeta(p, lang).title);
      expect(new Set(titles).size, lang).toBe(titles.length);
    }
  });

  it('官网页面(非控制台)都有长度合适的描述', () => {
    for (const [p, m] of Object.entries(ROUTE_META)) {
      if (m.app) continue;
      const zh = m.zh.description ?? '', en = m.en.description ?? '';
      expect(zh.length, `${p} zh`).toBeGreaterThanOrEqual(20);
      expect(zh.length, `${p} zh`).toBeLessThanOrEqual(90);
      expect(en.length, `${p} en`).toBeGreaterThanOrEqual(50);
      expect(en.length, `${p} en`).toBeLessThanOrEqual(160);
      expect(/[\u4e00-\u9fff]/.test(en), `${p} en 含中文`).toBe(false);
    }
  });

  it('标题格式:首页含品牌,其余 "页面 · Epicode",长度 ≤ 60', () => {
    expect(resolvePageMeta('/', 'zh').title).toBe('Epicode — AI 记忆操作系统');
    expect(resolvePageMeta('/docs', 'en').title).toBe('API Docs · Epicode');
    for (const p of routePaths) for (const lang of ['zh', 'en'] as const) {
      expect(resolvePageMeta(p, lang).title.length, `${p} ${lang}`).toBeLessThanOrEqual(60);
    }
  });
});

describe('resolvePageMeta', () => {
  it('规范化尾斜杠与查询串', () => {
    expect(normalizePath('/docs/')).toBe('/docs');
    expect(normalizePath('/dashboard/chat?q=hi')).toBe('/dashboard/chat');
    expect(normalizePath('')).toBe('/');
    expect(resolvePageMeta('/docs/', 'zh').title).toBe('API 文档 · Epicode');
  });

  it('未知路由给出 404 标题且不冒充首页', () => {
    const m = resolvePageMeta('/nope', 'en');
    expect(m.known).toBe(false);
    expect(m.title).toBe('Page not found · Epicode');
  });

  it('og:url 与 locale', () => {
    expect(resolvePageMeta('/', 'zh').url).toBe('https://epicode.cn/');
    expect(resolvePageMeta('/l0', 'en').url).toBe('https://epicode.cn/#/l0');
    expect(resolvePageMeta('/l0', 'en').locale).toBe('en_US');
    expect(resolvePageMeta('/l0', 'zh').locale).toBe('zh_CN');
  });

  it('isConsolePath 只匹配控制台', () => {
    expect(isConsolePath('/dashboard')).toBe(true);
    expect(isConsolePath('/dashboard/graph')).toBe(true);
    expect(isConsolePath('/docs')).toBe(false);
    expect(isConsolePath('/')).toBe(false);
  });
});

describe('applyPageMeta 写入 <head>', () => {
  it('更新 title / description / og,缺失的 meta 会被创建', () => {
    const metas: Record<string, { attrs: Record<string, string> }> = {};
    const mkEl = () => {
      const el = { attrs: {} as Record<string, string>, setAttribute(k: string, v: string) { el.attrs[k] = v; } };
      return el;
    };
    const head = {
      querySelector(sel: string) {
        const m = sel.match(/meta\[(name|property)="([^"]+)"\]/);
        return m ? Object.values(metas).find((e) => e.attrs[m[1]] === m[2]) ?? null : null;
      },
      appendChild(el: { attrs: Record<string, string> }) { metas[(el.attrs.name ?? el.attrs.property)!] = el; },
    };
    const g = globalThis as unknown as { document?: unknown };
    const prev = g.document;
    g.document = { title: '', head, createElement: mkEl };
    try {
      applyPageMeta('/benchmarks', 'zh');
      const doc = g.document as { title: string };
      expect(doc.title).toBe('性能基准 · Epicode');
      expect(currentPageTitle()).toBe('性能基准 · Epicode');
      expect(metas['description'].attrs.content).toContain('Recall@10');
      expect(metas['og:title'].attrs.content).toBe('性能基准 · Epicode');
      expect(metas['og:url'].attrs.content).toBe('https://epicode.cn/#/benchmarks');
      applyPageMeta('/benchmarks', 'en');
      expect(metas['og:locale'].attrs.content).toBe('en_US');
      expect(metas['og:locale:alternate'].attrs.content).toBe('zh_CN');
    } finally {
      g.document = prev;
    }
  });
});

describe('静态 <head> 与页脚', () => {
  const html = readFileSync('index.html', 'utf-8');
  it('index.html 有 canonical / og:site_name / twitter:card,JSON-LD 合法', () => {
    expect(html).toContain('<link rel="canonical" href="https://epicode.cn/" />');
    expect(html).toContain('property="og:site_name"');
    expect(html).toContain('name="twitter:card"');
    const ld = html.match(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/)![1];
    const data = JSON.parse(ld);
    expect(data['@type']).toBe('SoftwareApplication');
    expect(data.sameAs).toContain('https://github.com/sunormesky-max/epicode');
  });

  it('页脚不再链接到占位的 github.com / discord.com 首页,版本号不写死', () => {
    const footer = readFileSync('src/components/Footer.tsx', 'utf-8');
    expect(footer).not.toMatch(/href: 'https:\/\/github\.com'/);
    expect(footer).not.toMatch(/href: 'https:\/\/discord\.com'/);
    expect(footer).not.toContain("t('footer.version')");
    expect(footer).toContain("from '../../package.json'");
  });
});
