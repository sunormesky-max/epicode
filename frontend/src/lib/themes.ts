export const THEME_STORAGE_KEY = 'epicode-theme';

export type ThemeId = 'synapse' | 'paper' | 'meridian' | 'daylight' | 'amber' | 'archive';

export interface ThemeSpec {
  id: ThemeId;
  name: string;
  nameEn: string;
  summary: string;
  mode: 'dark' | 'light';
  swatches: [string, string, string, string];
}

export const THEMES: ThemeSpec[] = [
  {
    id: 'synapse',
    name: '突触青',
    nameEn: 'Synapse',
    summary: '默认研究所深墨。主动作色只有一枚青绿。',
    mode: 'dark',
    swatches: ['#07070a', '#101016', '#f5f4f0', '#3ecfae'],
  },
  {
    id: 'paper',
    name: '纸墨',
    nameEn: 'Paper',
    summary: '暖纸底、橄榄墨。官网适合长文，控制台适合白天。',
    mode: 'light',
    swatches: ['#f4efe6', '#fffaf3', '#1c1915', '#6b7c4a'],
  },
  {
    id: 'meridian',
    name: '经线',
    nameEn: 'Meridian',
    summary: '深空底、冷银强调。安静的夜间控制台。',
    mode: 'dark',
    swatches: ['#07090d', '#10151c', '#e7edf4', '#b7c4d4'],
  },
  {
    id: 'daylight',
    name: '昼白',
    nameEn: 'Daylight',
    summary: '冷白工作台。高对比，适合长时间看表格。',
    mode: 'light',
    swatches: ['#f6f7f9', '#ffffff', '#16181d', '#1f6f62'],
  },
  {
    id: 'amber',
    name: '琥珀暮',
    nameEn: 'Amber',
    summary: '暖黑底、琥珀动作色。夜间阅读和图谱都站得住。',
    mode: 'dark',
    swatches: ['#100c08', '#1a140f', '#f6efe4', '#e0a45a'],
  },
  {
    id: 'archive',
    name: '档案红',
    nameEn: 'Archive',
    summary: '墨底、朱红动作色。把记忆库读成一间档案室。',
    mode: 'dark',
    swatches: ['#0c090b', '#161014', '#f6efe8', '#d4535e'],
  },
];

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
  window.dispatchEvent(new CustomEvent('epicode-theme', { detail: id }));
}
