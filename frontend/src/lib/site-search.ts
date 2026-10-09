/**
 * 站内搜索(零依赖)— 官网页面 + API 端点。
 *
 * 数据全部来自现有的单一数据源:页面名复用 i18n key,端点来自 docs-endpoints.ts,
 * 所以文档新增端点后搜索自动可达,无需维护第二份索引。
 */
import type { Language, TranslationKey } from '@/i18n/translations';
import { API_SECTIONS, endpointAnchor, type Endpoint } from './docs-endpoints';

export type Translate = (key: TranslationKey) => string;
export type SearchKind = 'page' | 'endpoint' | 'resource';

export interface SearchEntry {
  id: string;
  kind: SearchKind;
  title: string;
  subtitle: string;
  href: string;
  /** 小写化的标题,用于前缀 / 子串加权 */
  titleN: string;
  /** 小写化的副标题(端点用途 / 分组),命中权重高于纯关键词 */
  subN: string;
  /** 小写化的全部可搜文本(标题 + 副标题 + 关键词,中英双语) */
  hay: string;
}

interface PageDef { path: string; key?: TranslationKey; label?: Record<Language, string>; kw: string }

// 关键词同时收录中英文,任一语言界面下都能用另一种语言搜到
const PAGES: PageDef[] = [
  { path: '/', key: 'nav.home', kw: 'home 首页 epicode memory os 记忆 操作系统' },
  { path: '/guide', key: 'nav.quickStart', kw: 'guide quick start onboarding tutorial api key 快速上手 入门 教程 接入' },
  { path: '/docs', key: 'nav.docs', kw: 'docs api reference rest endpoint 文档 接口 参考 端点' },
  { path: '/smrp', label: { zh: 'SMRP 协议', en: 'SMRP Protocol' }, kw: 'smrp structured memory response protocol envelope 响应协议 信封' },
  { path: '/l0', label: { zh: 'L0 协议', en: 'L0 Protocol' }, kw: 'l0 drive signal active inference grounding 主动推理 意志 驱动' },
  { path: '/community', key: 'nav.community', kw: 'community skills marketplace share 社区 技能 市场 共享' },
  { path: '/benchmarks', key: 'nav.benchmarks', kw: 'benchmarks performance latency qps recall mrr 基准 性能 延迟 召回' },
  { path: '/themes', label: { zh: '主题中心', en: 'Theme Center' }, kw: 'themes theme dark light accent colour color 主题 深色 浅色 配色 强调色' },
  { path: '/register', key: 'nav.getStarted', kw: 'register sign up create account free 注册 开始使用 免费' },
  { path: '/login', key: 'login.title', kw: 'login sign in console 登录 控制台' },
];

const RESOURCES: { href: string; title: string; kw: Record<Language, string> }[] = [
  { href: '/llms.txt', title: 'llms.txt', kw: { zh: '给 AI 读的完整站点说明(API、基准、协议)', en: 'Machine-readable site manifest for AI agents (API, benchmarks, protocols)' } },
  { href: '/ai.html', title: 'ai.html', kw: { zh: '零 JS 的 AI 专用页面', en: 'Zero-JS page for AI visitors' } },
];

export function norm(s: string): string {
  return s.normalize('NFKC').toLowerCase();
}

function entry(id: string, kind: SearchKind, title: string, subtitle: string, href: string, extra = ''): SearchEntry {
  return { id, kind, title, subtitle, href, titleN: norm(title), subN: norm(subtitle), hay: norm(`${title} ${subtitle} ${extra}`) };
}

export function buildSiteIndex(t: Translate, lang: Language): SearchEntry[] {
  const out: SearchEntry[] = [];
  for (const p of PAGES) {
    const title = p.key ? t(p.key) : p.label![lang];
    out.push(entry(`page:${p.path}`, 'page', title, `#${p.path}`, `#${p.path}`, p.kw));
  }
  for (const section of API_SECTIONS) {
    const sectionTitle = t(section.titleKey as TranslationKey);
    for (const ep of section.endpoints) {
      const desc = t(ep.descKey as TranslationKey);
      out.push(entry(
        `ep:${endpointAnchor(ep)}`, 'endpoint', `${ep.method} ${ep.path}`, `${desc} · ${sectionTitle}`,
        `#/docs?ep=${endpointAnchor(ep)}`, `${ep.auth ? 'auth' : 'public'} ${section.titleKey}`,
      ));
    }
  }
  for (const r of RESOURCES) out.push(entry(`res:${r.href}`, 'resource', r.title, r.kw[lang], r.href, r.kw.zh + ' ' + r.kw.en));
  return out;
}

/**
 * 多词 AND 匹配;标题前缀 > 标题子串 > 其它文本。完全等于标题再加分。
 * 纯函数,便于单测。
 */
export function searchSite(index: SearchEntry[], query: string, limit = 12): SearchEntry[] {
  const q = norm(query).trim();
  if (!q) return [];
  const tokens = q.split(/\s+/).filter(Boolean);
  const scored: { e: SearchEntry; score: number }[] = [];
  for (const e of index) {
    let score = 0;
    let ok = true;
    for (const tok of tokens) {
      const ti = e.titleN.indexOf(tok);
      // 端点标题形如 "POST /v1/search":路径段开头也算前缀
      if (ti === 0 || (ti > 0 && /[\s/]/.test(e.titleN[ti - 1]))) score += 30;
      else if (ti > 0) score += 15;
      else if (e.subN.includes(tok)) score += 6;
      // 只命中隐藏关键词(如首页的"记忆")排在真正描述该词的条目之后
      else if (e.hay.includes(tok)) score += 4;
      else { ok = false; break; }
    }
    if (!ok) continue;
    if (e.titleN === q) score += 50;
    if (e.kind === 'page' && score >= 15 * tokens.length) score += 3;
    scored.push({ e, score });
  }
  scored.sort((a, b) => b.score - a.score || a.e.title.length - b.e.title.length || a.e.title.localeCompare(b.e.title));
  return scored.slice(0, limit).map((s) => s.e);
}

/** Docs 页面内筛选:返回每个分组中命中的端点(分组保持原顺序;未命中的分组 endpoints 为空) */
export function filterEndpoints<S extends { titleKey: string; endpoints: Endpoint[] }>(
  sections: S[], query: string, t: Translate,
): { section: S; endpoints: Endpoint[] }[] {
  const tokens = norm(query).trim().split(/\s+/).filter(Boolean);
  return sections.map((section) => {
    if (!tokens.length) return { section, endpoints: section.endpoints };
    const st = norm(t(section.titleKey as TranslationKey));
    return {
      section,
      endpoints: section.endpoints.filter((ep) => {
        const hay = `${norm(ep.method)} ${norm(ep.path)} ${norm(t(ep.descKey as TranslationKey))} ${st}`;
        return tokens.every((tok) => hay.includes(tok));
      }),
    };
  });
}

/** 打开搜索结果:站内 hash 路由或静态资源(llms.txt 等) */
export function navigateTo(href: string): void {
  if (href.startsWith('#')) window.location.hash = href.slice(1);
  else window.location.assign(href);
}
