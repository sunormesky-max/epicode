export const THEME_STORAGE_KEY = 'epicode-theme';

export type ThemeId =
  | 'synapse' | 'paper' | 'meridian' | 'daylight' | 'amber' | 'archive'
  | 'x-dark' | 'x-light' | 'x-dim';

export interface ThemeSpec {
  id: ThemeId;
  name: string;
  nameEn: string;
  summary: string;
  summaryEn: string;
  mode: 'dark' | 'light';
  swatches: [string, string, string, string];
}

export const THEMES: ThemeSpec[] = [
  {
    id: 'synapse',
    name: '突触青',
    nameEn: 'Synapse',
    summary: '默认研究所深墨。主动作色只有一枚青绿。',
    summaryEn: 'Default deep-ink research lab. A single teal action colour.',
    mode: 'dark',
    swatches: ['#07070a', '#101016', '#f5f4f0', '#3ecfae'],
  },
  {
    id: 'paper',
    name: '纸墨',
    nameEn: 'Paper',
    summary: '暖纸底、橄榄墨。官网适合长文，控制台适合白天。',
    summaryEn: 'Warm paper and olive ink. Good for long reading on the site and daytime console work.',
    mode: 'light',
    swatches: ['#f4efe6', '#fffaf3', '#1c1915', '#5e6c42'],
  },
  {
    id: 'meridian',
    name: '经线',
    nameEn: 'Meridian',
    summary: '深空底、冷银强调。安静的夜间控制台。',
    summaryEn: 'Deep-space base with cool silver accents. A quiet night console.',
    mode: 'dark',
    swatches: ['#07090d', '#10151c', '#e7edf4', '#b7c4d4'],
  },
  {
    id: 'daylight',
    name: '昼白',
    nameEn: 'Daylight',
    summary: '冷白工作台。高对比，适合长时间看表格。',
    summaryEn: 'Cool white workbench. High contrast, built for long hours with tables.',
    mode: 'light',
    swatches: ['#f6f7f9', '#ffffff', '#16181d', '#1f6f62'],
  },
  {
    id: 'amber',
    name: '琥珀暮',
    nameEn: 'Amber',
    summary: '暖黑底、琥珀动作色。夜间阅读和图谱都站得住。',
    summaryEn: 'Warm black with amber actions. Holds up for night reading and the graph.',
    mode: 'dark',
    swatches: ['#100c08', '#1a140f', '#f6efe4', '#e0a45a'],
  },
  {
    id: 'archive',
    name: '档案红',
    nameEn: 'Archive',
    summary: '墨底、朱红动作色。把记忆库读成一间档案室。',
    summaryEn: 'Ink base with vermilion actions. Reads the memory store as an archive room.',
    mode: 'dark',
    swatches: ['#0c090b', '#161014', '#f6efe8', '#d55661'],
  },
  {
    id: 'x-dark',
    name: 'X 深黑',
    nameEn: 'X Lights Out',
    summary: 'X（原 Twitter）夜间模式：纯黑底、#16181C 卡片、胶囊按钮与 X 蓝。',
    summaryEn: 'X (formerly Twitter) Lights Out: pure black, #16181C cards, pill buttons and X blue.',
    mode: 'dark',
    swatches: ['#000000', '#16181c', '#e7e9ea', '#1d9bf0'],
  },
  {
    id: 'x-light',
    name: 'X 浅色',
    nameEn: 'X Light',
    summary: 'X（原 Twitter）白天模式：白底、#0F1419 文字；强调蓝按 WCAG AA 加深。',
    summaryEn: 'X (formerly Twitter) Light: white base, #0F1419 text; accent blue deepened to meet WCAG AA.',
    mode: 'light',
    swatches: ['#ffffff', '#f7f9f9', '#0f1419', '#1d9bf0'],
  },
  {
    id: 'x-dim',
    name: 'X 暗蓝灰',
    nameEn: 'X Dim',
    summary: 'X（原 Twitter）Dim：#15202B 暗蓝灰底，比纯黑更柔和。',
    summaryEn: 'X (formerly Twitter) Dim: #15202B blue-grey base, softer than pure black.',
    mode: 'dark',
    swatches: ['#15202b', '#1e2732', '#f7f9f9', '#1d9bf0'],
  },
];

/** 按界面语言取主题名称/简介(走现有 I18nContext 的 lang)。 */
export function themeName(theme: ThemeSpec, lang: 'zh' | 'en'): string {
  return lang === 'en' ? theme.nameEn : theme.name;
}

export function themeSummary(theme: ThemeSpec, lang: 'zh' | 'en'): string {
  return lang === 'en' ? theme.summaryEn : theme.summary;
}

export function isThemeId(value: string | null): value is ThemeId {
  return THEMES.some((theme) => theme.id === value);
}

export function readStoredTheme(): ThemeId {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (isThemeId(stored)) return stored;
  } catch {
    /* private mode */
  }
  return 'synapse';
}

export function applyTheme(id: ThemeId) {
  const theme = THEMES.find((item) => item.id === id) ?? THEMES[0];
  document.documentElement.dataset.theme = theme.id;
  document.documentElement.style.colorScheme = theme.mode;
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.setAttribute('content', theme.swatches[0]);
}

export function setTheme(id: ThemeId) {
  applyTheme(id);
  try {
    localStorage.setItem(THEME_STORAGE_KEY, id);
  } catch {
    /* ignore */
  }
  // 手动选主题即表示"固定用这一套":关闭跟随系统。
  const prefs = readThemePrefs();
  if (prefs.followSystem) writeThemePrefs({ ...prefs, followSystem: false });
  window.dispatchEvent(new CustomEvent('epicode-theme', { detail: id }));
}

/* ---------------- 主题管理中心:偏好(跟随系统 / 背景动画) ---------------- */

export const THEME_PREFS_KEY = 'epicode-theme-prefs';

export type ThemeFamily = 'classic' | 'x';

export function themeFamily(theme: ThemeSpec): ThemeFamily {
  return theme.id.startsWith('x-') ? 'x' : 'classic';
}

export interface ThemePrefs {
  /** 跟随系统浅色/深色自动切换 */
  followSystem: boolean;
  /** 跟随系统时,系统为浅色用的主题 */
  light: ThemeId;
  /** 跟随系统时,系统为深色用的主题 */
  dark: ThemeId;
  /** 星海神经动态网络背景开关 */
  bgEnabled: boolean;
  /** 背景强度 0–100,乘在主题自带的 --nn-opacity 上 */
  bgIntensity: number;
}

export const DEFAULT_THEME_PREFS: ThemePrefs = {
  followSystem: false,
  light: 'x-light',
  dark: 'x-dark',
  bgEnabled: true,
  bgIntensity: 100,
};

function modeOf(id: ThemeId): 'dark' | 'light' {
  return THEMES.find((t) => t.id === id)?.mode ?? 'dark';
}

/** 解析并校验存储的偏好;任何非法字段都回落到默认值。 */
export function parseThemePrefs(raw: string | null): ThemePrefs {
  if (!raw) return { ...DEFAULT_THEME_PREFS };
  let data: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return { ...DEFAULT_THEME_PREFS };
    data = parsed as Record<string, unknown>;
  } catch {
    return { ...DEFAULT_THEME_PREFS };
  }
  const pick = (value: unknown, mode: 'dark' | 'light', fallback: ThemeId): ThemeId =>
    typeof value === 'string' && isThemeId(value) && modeOf(value) === mode ? value : fallback;
  const intensity = typeof data.bgIntensity === 'number' && Number.isFinite(data.bgIntensity)
    ? Math.round(Math.min(100, Math.max(0, data.bgIntensity)))
    : DEFAULT_THEME_PREFS.bgIntensity;
  return {
    followSystem: data.followSystem === true,
    light: pick(data.light, 'light', DEFAULT_THEME_PREFS.light),
    dark: pick(data.dark, 'dark', DEFAULT_THEME_PREFS.dark),
    bgEnabled: data.bgEnabled !== false,
    bgIntensity: intensity,
  };
}

/** 决定当前应生效的主题:跟随系统时取对应槽位,否则用手动选择。 */
export function resolveTheme(prefs: ThemePrefs, stored: ThemeId, systemDark: boolean): ThemeId {
  if (!prefs.followSystem) return stored;
  return systemDark ? prefs.dark : prefs.light;
}

export function readThemePrefs(): ThemePrefs {
  try {
    return parseThemePrefs(localStorage.getItem(THEME_PREFS_KEY));
  } catch {
    return { ...DEFAULT_THEME_PREFS };
  }
}

export function writeThemePrefs(prefs: ThemePrefs) {
  try {
    localStorage.setItem(THEME_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    /* ignore */
  }
}

const darkQuery = () =>
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-color-scheme: dark)')
    : null;

/** 把背景偏好写到 <html>:data-nn-bg 控制开关,--nn-intensity 控制强度。 */
export function applyBackgroundPrefs(prefs: ThemePrefs) {
  const root = document.documentElement;
  root.dataset.nnBg = prefs.bgEnabled ? 'on' : 'off';
  root.style.setProperty('--nn-intensity', String(prefs.bgIntensity / 100));
}

/** 当前实际生效的主题(跟随系统时可能与手动存储的不同)。 */
export function activeTheme(): ThemeId {
  const current = document.documentElement.dataset.theme ?? null;
  return isThemeId(current) ? current : readStoredTheme();
}

/** 按偏好重新计算并应用主题,广播 epicode-theme 让切换器同步。 */
export function applyThemePrefs(prefs: ThemePrefs = readThemePrefs()) {
  const id = resolveTheme(prefs, readStoredTheme(), darkQuery()?.matches ?? true);
  applyTheme(id);
  applyBackgroundPrefs(prefs);
  window.dispatchEvent(new CustomEvent('epicode-theme', { detail: id }));
  return id;
}

export function updateThemePrefs(patch: Partial<ThemePrefs>) {
  const next = { ...readThemePrefs(), ...patch };
  writeThemePrefs(next);
  return { prefs: next, theme: applyThemePrefs(next) };
}

/** 恢复默认:突触青 + 背景开启 100% + 不跟随系统。 */
export function resetThemePrefs() {
  try {
    localStorage.removeItem(THEME_PREFS_KEY);
    localStorage.removeItem(THEME_STORAGE_KEY);
  } catch {
    /* ignore */
  }
  return applyThemePrefs({ ...DEFAULT_THEME_PREFS });
}

/** 启动时调用:首屏前应用主题与背景偏好,并监听系统浅/深色变化。 */
export function initTheme() {
  const prefs = readThemePrefs();
  applyTheme(resolveTheme(prefs, readStoredTheme(), darkQuery()?.matches ?? true));
  applyBackgroundPrefs(prefs);
  darkQuery()?.addEventListener('change', () => {
    if (readThemePrefs().followSystem) applyThemePrefs();
  });
}
