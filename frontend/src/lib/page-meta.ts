/**
 * 路由级页面元信息(标题 / 描述 / og)— 中英双语。
 *
 * 站点使用 HashRouter,所有页面共享同一个 index.html:之前任何页面的 <title> 和
 * meta description 都是首页的英文文案,浏览器标签页、历史记录、书签、分享卡片
 * 和读屏器(WCAG 2.4.2 Page Titled)都无法区分页面。这里集中维护每个路由的文案,
 * 由 App 在路由或语言变化时调用 applyPageMeta 同步到 <head>。
 */
import type { Language } from '@/i18n/translations';

export const SITE_NAME = 'Epicode';
export const SITE_ORIGIN = 'https://epicode.cn';
export const REPO_URL = 'https://github.com/sunormesky-max/epicode';

interface LocalizedMeta { title: string; description?: string }
export interface PageMeta { zh: LocalizedMeta; en: LocalizedMeta; /** 控制台页:不参与 SEO,仅用于标签页标题 */ app?: boolean }

const HOME: PageMeta = {
  zh: {
    title: 'Epicode — AI 记忆操作系统',
    description: '给 AI 一段忘不掉的记忆:持久记忆、语义检索、知识图谱、双环认知与技能交换,提供 REST API 与 41 个 MCP 工具。',
  },
  en: {
    title: 'Epicode — AI Memory Operating System',
    description: 'Give AI an unforgettable memory: persistent memory, semantic search, knowledge graph, dual-loop cognition and skill exchange, via REST API and 41 MCP tools.',
  },
};

export const ROUTE_META: Record<string, PageMeta> = {
  '/': HOME,
  '/guide': {
    zh: { title: '快速上手', description: '5 步完成 Epicode 集成:获取 API Key、存储第一条记忆、语义搜索、使用技能系统、确认 AI 身份。' },
    en: { title: 'Quick Start', description: 'Integrate Epicode in 5 steps: get an API key, store your first memory, run semantic search, use skills, and confirm the AI identity.' },
  },
  '/docs': {
    zh: { title: 'API 文档', description: 'Epicode REST API 参考:认证、记忆写入与检索、文档导入、统计与图谱、实时事件流、技能与 MCP 端点。' },
    en: { title: 'API Docs', description: 'Epicode REST API reference: auth, memory store and search, document import, stats and graph, realtime stream, skills and MCP endpoints.' },
  },
  '/benchmarks': {
    zh: { title: '性能基准', description: 'Epicode 在资源受限环境下的真实表现:检索延迟、吞吐、召回率(Recall@10 / MRR)与线上公开统计。' },
    en: { title: 'Benchmarks', description: 'How Epicode performs on constrained hardware: search latency, throughput, retrieval quality (Recall@10 / MRR) and live public stats.' },
  },
  '/smrp': {
    zh: { title: 'SMRP 协议', description: 'SMRP(Structured Memory Response Protocol)1.0:统一的记忆响应信封、分层结果与来源拓扑,独立于传输层。' },
    en: { title: 'SMRP Protocol', description: 'SMRP (Structured Memory Response Protocol) 1.0: one memory-response envelope with tiers and source topology, independent of transport.' },
  },
  '/l0': {
    zh: { title: 'L0 协议', description: 'L0 主动推理协议:记忆驱动的 Drive Signal、证据溯源(grounding)、投递窗口与执行反馈闭环。' },
    en: { title: 'L0 Protocol', description: 'L0 active-inference protocol: memory-driven drive signals, grounding, delivery windows and the execution-feedback loop.' },
  },
  '/community': {
    zh: { title: '社区技能', description: '浏览并拉取社区共享的 Epicode 技能,让你的 AI 代理一键复用他人的工作流。' },
    en: { title: 'Community Skills', description: 'Browse and pull community-shared Epicode skills so your AI agents can reuse proven workflows in one click.' },
  },
  '/themes': {
    zh: { title: '主题中心', description: '为 Epicode 官网与控制台选择配色主题与强调色,支持跟随系统浅色 / 深色。' },
    en: { title: 'Theme Center', description: 'Pick a colour theme and accent for the Epicode site and console, with optional follow-system light / dark.' },
  },
  '/login': {
    zh: { title: '登录', description: '登录 Epicode 控制台,管理你的 AI 记忆空间。' },
    en: { title: 'Sign in', description: 'Sign in to the Epicode console to manage your AI memory space.' },
  },
  '/register': {
    zh: { title: '注册', description: '免费注册 Epicode,几分钟内为你的 AI 接入持久记忆。' },
    en: { title: 'Create account', description: 'Create a free Epicode account and give your AI persistent memory in minutes.' },
  },
  // 控制台(需登录,仅标签页标题)
  '/dashboard': { app: true, zh: { title: '总览 · 控制台' }, en: { title: 'Overview · Console' } },
  '/dashboard/memories': { app: true, zh: { title: '记忆 · 控制台' }, en: { title: 'Memories · Console' } },
  '/dashboard/chat': { app: true, zh: { title: '对话 · 控制台' }, en: { title: 'Chat · Console' } },
  '/dashboard/graph': { app: true, zh: { title: '知识图谱 · 控制台' }, en: { title: 'Graph · Console' } },
  '/dashboard/archive': { app: true, zh: { title: '档案库 · 控制台' }, en: { title: 'Archive · Console' } },
  '/dashboard/cognitive': { app: true, zh: { title: '认知引擎 · 控制台' }, en: { title: 'Cognitive · Console' } },
  '/dashboard/observe': { app: true, zh: { title: '观测舱 · 控制台' }, en: { title: 'Observe · Console' } },
  '/dashboard/skills': { app: true, zh: { title: '技能 · 控制台' }, en: { title: 'Skills · Console' } },
  '/dashboard/library': { app: true, zh: { title: '图书馆 · 控制台' }, en: { title: 'Library · Console' } },
  '/dashboard/accounts': { app: true, zh: { title: '子账户 · 控制台' }, en: { title: 'Sub-accounts · Console' } },
  '/dashboard/themes': { app: true, zh: { title: '主题 · 控制台' }, en: { title: 'Themes · Console' } },
  '/dashboard/permissions': { app: true, zh: { title: '权限 · 控制台' }, en: { title: 'Permissions · Console' } },
};

const NOT_FOUND: PageMeta = {
  zh: { title: '页面不存在', description: HOME.zh.description },
  en: { title: 'Page not found', description: HOME.en.description },
};

export interface ResolvedMeta { title: string; description: string; url: string; locale: string; known: boolean }

export function normalizePath(pathname: string): string {
  const p = (pathname || '/').split(/[?#]/)[0].replace(/\/+$/, '');
  return p === '' ? '/' : p;
}

export function resolvePageMeta(pathname: string, lang: Language): ResolvedMeta {
  const path = normalizePath(pathname);
  const entry = ROUTE_META[path];
  const meta = entry ?? NOT_FOUND;
  const loc = meta[lang] ?? meta.zh;
  const home = HOME[lang];
  return {
    // 首页标题本身已含品牌;其余页面统一 "页面 · Epicode"
    title: path === '/' ? home.title : `${loc.title} · ${SITE_NAME}`,
    description: loc.description ?? home.description!,
    url: path === '/' ? `${SITE_ORIGIN}/` : `${SITE_ORIGIN}/#${path}`,
    locale: lang === 'zh' ? 'zh_CN' : 'en_US',
    known: Boolean(entry),
  };
}

/** 当前路由的标题;实时状态(SSE)恢复标题时使用,避免回落成挂载时的旧标题 */
let currentTitle = typeof document !== 'undefined' ? document.title : HOME.en.title;
export function currentPageTitle(): string { return currentTitle; }

function setMeta(attr: 'name' | 'property', key: string, content: string): void {
  let el = document.head.querySelector<HTMLMetaElement>(`meta[${attr}="${key}"]`);
  if (!el) {
    el = document.createElement('meta');
    el.setAttribute(attr, key);
    document.head.appendChild(el);
  }
  el.setAttribute('content', content);
}

export function applyPageMeta(pathname: string, lang: Language): ResolvedMeta {
  const m = resolvePageMeta(pathname, lang);
  currentTitle = m.title;
  document.title = m.title;
  setMeta('name', 'description', m.description);
  setMeta('property', 'og:title', m.title);
  setMeta('property', 'og:description', m.description);
  setMeta('property', 'og:url', m.url);
  setMeta('property', 'og:locale', m.locale);
  setMeta('property', 'og:locale:alternate', lang === 'zh' ? 'en_US' : 'zh_CN');
  setMeta('name', 'twitter:title', m.title);
  setMeta('name', 'twitter:description', m.description);
  return m;
}

/** 控制台路由会把标题交给实时认知状态(SSE)显示;官网页面保留 SEO 标题 */
export function isConsolePath(pathname: string): boolean {
  return normalizePath(pathname).startsWith('/dashboard');
}
