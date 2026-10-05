import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { DEFAULT_THEME_PREFS, THEMES, parseThemePrefs, resolveTheme, themeFamily, themeName, themeSummary } from '../themes';

const css = readFileSync(new URL('../../index.css', import.meta.url), 'utf8');

describe('themes registry', () => {
  it('has unique ids and bilingual copy for every theme', () => {
    expect(new Set(THEMES.map((t) => t.id)).size).toBe(THEMES.length);
    for (const theme of THEMES) {
      expect(theme.name.length).toBeGreaterThan(0);
      expect(theme.nameEn.length).toBeGreaterThan(0);
      expect(theme.summary.length).toBeGreaterThan(0);
      expect(theme.summaryEn.length).toBeGreaterThan(0);
      expect(themeName(theme, 'en')).toBe(theme.nameEn);
      expect(themeName(theme, 'zh')).toBe(theme.name);
      expect(themeSummary(theme, 'en')).toBe(theme.summaryEn);
      expect(themeSummary(theme, 'zh')).toBe(theme.summary);
    }
  });

  it('uses valid, distinct swatch colours', () => {
    for (const theme of THEMES) {
      expect(new Set(theme.swatches).size).toBe(4);
      for (const color of theme.swatches) expect(color).toMatch(/^#[0-9a-f]{6}$/);
    }
  });

  it('defines a CSS variable block for every non-default theme', () => {
    for (const theme of THEMES.filter((t) => t.id !== 'synapse')) {
      expect(css).toContain(`html[data-theme="${theme.id}"] {`);
    }
  });

  it('keeps the neural-network background for X themes (colours via --nn-* variables)', () => {
    expect(css).not.toContain('--sacred-fx');
    for (const id of ['x-dark', 'x-light', 'x-dim']) {
      const start = css.indexOf(`html[data-theme="${id}"] {`);
      const block = css.slice(start, css.indexOf('}', start));
      for (const name of ['--nn-primary', '--nn-hi', '--nn-secondary', '--nn-opacity']) {
        expect(block).toContain(name);
      }
    }
  });
});

describe('theme center preferences', () => {
  it('falls back to defaults for missing or malformed prefs', () => {
    expect(parseThemePrefs(null)).toEqual(DEFAULT_THEME_PREFS);
    expect(parseThemePrefs('not json')).toEqual(DEFAULT_THEME_PREFS);
    expect(parseThemePrefs('42')).toEqual(DEFAULT_THEME_PREFS);
  });

  it('keeps background on at 100% by default so existing behaviour is unchanged', () => {
    expect(DEFAULT_THEME_PREFS.bgEnabled).toBe(true);
    expect(DEFAULT_THEME_PREFS.bgIntensity).toBe(100);
    expect(DEFAULT_THEME_PREFS.followSystem).toBe(false);
  });

  it('sanitises stored values', () => {
    const prefs = parseThemePrefs(JSON.stringify({
      followSystem: true, light: 'x-dark', dark: 'nope', bgEnabled: false, bgIntensity: 250,
    }));
    expect(prefs.followSystem).toBe(true);
    expect(prefs.light).toBe(DEFAULT_THEME_PREFS.light); // dark theme rejected for light slot
    expect(prefs.dark).toBe(DEFAULT_THEME_PREFS.dark);
    expect(prefs.bgEnabled).toBe(false);
    expect(prefs.bgIntensity).toBe(100);
    expect(parseThemePrefs(JSON.stringify({ bgIntensity: -5 })).bgIntensity).toBe(0);
    expect(parseThemePrefs(JSON.stringify({ light: 'paper', dark: 'amber' }))).toMatchObject({ light: 'paper', dark: 'amber' });
  });

  it('resolves the active theme from follow-system prefs', () => {
    const follow = { ...DEFAULT_THEME_PREFS, followSystem: true, light: 'daylight' as const, dark: 'x-dim' as const };
    expect(resolveTheme(follow, 'amber', true)).toBe('x-dim');
    expect(resolveTheme(follow, 'amber', false)).toBe('daylight');
    expect(resolveTheme({ ...follow, followSystem: false }, 'amber', false)).toBe('amber');
  });

  it('groups themes into classic and X families', () => {
    expect(THEMES.filter((t) => themeFamily(t) === 'x').map((t) => t.id)).toEqual(['x-dark', 'x-light', 'x-dim']);
    expect(THEMES.filter((t) => themeFamily(t) === 'classic')).toHaveLength(6);
  });
});
