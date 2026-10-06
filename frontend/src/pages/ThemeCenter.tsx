import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { useLocation } from 'react-router';
import Layout from '@/components/Layout';
import DashboardLayout from '@/components/DashboardLayout';
import {
  THEMES,
  activeTheme,
  readThemePrefs,
  resetThemePrefs,
  setTheme,
  themeFamily,
  themeName,
  themeSummary,
  updateThemePrefs,
  type ThemeFamily,
  type ThemeId,
  type ThemePrefs,
  type ThemeSpec,
} from '@/lib/themes';
import { getUserSettings, setUserSettings, type UserSettings } from '@/lib/api';
import { useI18nContext } from '@/i18n/useI18n';

const mono: CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.12em', color: 'var(--text-tertiary)' };

const panel: CSSProperties = {
  background: 'var(--bg-card-solid)',
  border: '1px solid var(--border-light)',
  borderRadius: 16,
  padding: '18px 20px',
};

const selectStyle: CSSProperties = {
  background: 'var(--bg-card-solid)',
  color: 'var(--text-primary)',
  border: '1px solid var(--border-light)',
  borderRadius: 999,
  padding: '6px 12px',
  fontSize: 12,
};

function Switch({ checked, onChange, label }: { checked: boolean; onChange: (next: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      style={{
        width: 42,
        height: 24,
        flex: '0 0 auto',
        borderRadius: 999,
        border: '1px solid var(--border-light)',
        background: checked ? 'var(--accent-cyan)' : 'var(--bg-card)',
        position: 'relative',
        cursor: 'pointer',
        transition: 'background 0.15s',
      }}
    >
      <span
        style={{
          position: 'absolute',
          top: 2,
          left: checked ? 20 : 2,
          width: 18,
          height: 18,
          borderRadius: '50%',
          background: checked ? 'var(--bg-primary, #fff)' : 'var(--text-tertiary)',
          transition: 'left 0.15s',
        }}
      />
    </button>
  );
}

function Row({ title, hint, control, children }: { title: string; hint: string; control: ReactNode; children?: ReactNode }) {
  return (
    <div style={{ padding: '14px 0', borderTop: '1px solid var(--border-light)' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16 }}>
        <div>
          <strong style={{ fontSize: 14 }}>{title}</strong>
          <p style={{ color: 'var(--text-secondary)', fontSize: 12.5, lineHeight: 1.6, marginTop: 2 }}>{hint}</p>
        </div>
        {control}
      </div>
      {children}
    </div>
  );
}

/** 卡片里的迷你界面:主题底色 + 卡片面 + 文字行 + 强调色胶囊按钮。 */
function MiniPreview({ theme }: { theme: ThemeSpec }) {
  const [bg, surface, text, accent] = theme.swatches;
  return (
    <div aria-hidden style={{ background: bg, borderRadius: 10, padding: 10, height: 86, marginBottom: 12, border: '1px solid rgba(127,127,127,0.18)' }}>
      <div style={{ background: surface, borderRadius: 8, height: '100%', padding: '9px 10px', display: 'flex', flexDirection: 'column', gap: 6, border: '1px solid rgba(127,127,127,0.14)' }}>
        <span style={{ display: 'block', width: '62%', height: 6, borderRadius: 3, background: text }} />
        <span style={{ display: 'block', width: '86%', height: 5, borderRadius: 3, background: text, opacity: 0.45 }} />
        <span style={{ display: 'block', width: '48%', height: 5, borderRadius: 3, background: text, opacity: 0.45 }} />
        <span style={{ marginTop: 'auto', alignSelf: 'flex-start', width: 46, height: 14, borderRadius: 999, background: accent }} />
      </div>
    </div>
  );
}

export default function ThemeCenter() {
  const { lang } = useI18nContext();
  const zh = lang === 'zh';
  const [current, setCurrent] = useState<ThemeId>(() => activeTheme());
  const [prefs, setPrefs] = useState<ThemePrefs>(() => readThemePrefs());
  const location = useLocation();
  const inConsole = location.pathname.startsWith('/dashboard');
  const [, setAccountSettings] = useState<UserSettings | null>(null);
  const [canCustom, setCanCustom] = useState(false);
  const [planLabel, setPlanLabel] = useState('');
  const [cssDraft, setCssDraft] = useState('');
  const [saveMsg, setSaveMsg] = useState('');
  const [saveErr, setSaveErr] = useState('');

  useEffect(() => {
    if (!inConsole) return;
    let mounted = true;
    getUserSettings()
      .then((d) => {
        if (!mounted) return;
        setAccountSettings(d.settings || {});
        setCanCustom(!!d.can_theme_custom);
        setPlanLabel(d.plan || '');
        setCssDraft((d.settings?.theme_custom_css as string) || '');
        const serverTheme = d.settings?.theme as ThemeId | undefined;
        if (serverTheme && serverTheme !== activeTheme()) {
          setTheme(serverTheme);
          setCurrent(serverTheme);
          setPrefs(readThemePrefs());
        }
      })
      .catch(() => {});
    return () => { mounted = false; };
  }, [inConsole]);

  useEffect(() => {
    const onChange = (event: Event) => {
      const next = (event as CustomEvent<ThemeId>).detail;
      if (next) setCurrent(next);
      setPrefs(readThemePrefs());
    };
    window.addEventListener('epicode-theme', onChange);
    return () => window.removeEventListener('epicode-theme', onChange);
  }, []);

  const Shell = inConsole ? DashboardLayout : Layout;
  const update = (patch: Partial<ThemePrefs>) => {
    const result = updateThemePrefs(patch);
    setPrefs(result.prefs);
    setCurrent(result.theme);
  };
  const pick = (id: ThemeId) => {
    setTheme(id);
    setCurrent(id);
    setPrefs(readThemePrefs());
    if (inConsole) {
      setUserSettings({ theme: id }).catch(() => {});
    }
  };

  const saveCustomCss = async () => {
    setSaveMsg(''); setSaveErr('');
    try {
      const r = await setUserSettings({ theme_custom_css: cssDraft });
      setAccountSettings(r.settings);
      setSaveMsg(zh ? '已保存到账户 ✓' : 'Saved ✓');
      const styleId = 'epicode-custom-css';
      document.getElementById(styleId)?.remove();
      if (cssDraft.trim()) {
        const el = document.createElement('style');
        el.id = styleId;
        el.textContent = cssDraft;
        document.head.appendChild(el);
      }
    } catch (e) {
      setSaveErr(e instanceof Error ? e.message : String(e));
    }
  };

  const groups: { family: ThemeFamily; title: string; hint: string }[] = [
    { family: 'classic', title: zh ? '经典系列' : 'Classic', hint: zh ? 'Epicode 原生的六套研究所配色。' : 'The six original Epicode research-lab palettes.' },
    { family: 'x', title: zh ? 'X 系列' : 'X series', hint: zh ? '参照 X(原 Twitter)的深黑、暗蓝灰与浅色模式:极简、细边框、蓝色强调。' : 'Modelled on X (formerly Twitter): Lights Out, Dim and Light. Minimal, hairline borders, blue accent.' },
  ];
  const lights = THEMES.filter((t) => t.mode === 'light');
  const darks = THEMES.filter((t) => t.mode === 'dark');

  return (
    <Shell>
      <main style={{ maxWidth: 980, margin: '0 auto', padding: inConsole ? '2rem 1.25rem 4rem' : '7.5rem 1.25rem 4rem', position: 'relative', zIndex: 1 }}>
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, letterSpacing: '0.16em', color: 'var(--accent-cyan)' }}>THEME CENTER</p>
        <h1 style={{ fontFamily: 'var(--font-display)', fontSize: 'clamp(2rem, 5vw, 3.4rem)', margin: '0.4rem 0 0.6rem' }}>{zh ? '主题管理中心' : 'Theme Center'}</h1>
        <p style={{ maxWidth: 640, color: 'var(--text-secondary)', lineHeight: 1.7 }}>
          {zh
            ? `官网和控制台共用这一套选择。${inConsole ? '控制台内主题跟随账户设定，初始用户默认 Synapse 原始主题。' : '设置保存在这台浏览器里。'}`
            : 'The site and the console share one choice. A theme only changes colour, surfaces and accents, never the information structure. All settings are saved in this browser.'}
        </p>

        {inConsole && (
          <section aria-label={zh ? 'Custom theme' : 'Custom theme'} style={{ ...panel, marginTop: 28, opacity: canCustom ? 1 : 0.72 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', paddingBottom: 12 }}>
              <span style={mono}>{zh ? '自定义主题 (跟随账户)' : 'CUSTOM THEME (follows account)'}</span>
              <span style={{ ...mono, color: canCustom ? 'var(--success-green)' : 'var(--warning-orange)' }}>
                {planLabel ? planLabel.toUpperCase() + ' ' : ''}{canCustom ? (zh ? '可用' : 'enabled') : (zh ? '免费版仅可切换内置主题' : 'Free: built-in only')}
              </span>
            </div>
            <p style={{ color: 'var(--text-tertiary)', fontSize: 12.5, lineHeight: 1.7, margin: '0 0 10px' }}>
              {zh ? '上传/粘贴自定义 CSS 覆盖主题 token。保存后跟随账户, 所有设备生效。' : 'Paste custom CSS to override theme tokens. Saved to your account.'}
            </p>
            <textarea
              value={cssDraft}
              onChange={(e) => { setCssDraft(e.target.value); setSaveMsg(''); setSaveErr(''); }}
              disabled={!canCustom}
              placeholder={canCustom ? ':root { --accent-cyan: #ff6b6b; }' : (zh ? '升级到 Pro 后解锁' : 'Upgrade to Pro')}
              rows={7}
              style={{
                width: '100%', boxSizing: 'border-box',
                background: 'rgba(0,0,0,0.35)', color: 'var(--text-primary)',
                border: '1px solid var(--border-light)', borderRadius: 10,
                padding: '10px 12px', fontFamily: 'var(--font-mono)', fontSize: 12,
                resize: 'vertical', cursor: canCustom ? 'text' : 'not-allowed',
              }}
            />
            <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginTop: 10, flexWrap: 'wrap' }}>
              <button
                type="button"
                onClick={saveCustomCss}
                disabled={!canCustom}
                style={{ ...selectStyle, cursor: canCustom ? 'pointer' : 'not-allowed', opacity: canCustom ? 1 : 0.5 }}
              >
                {zh ? '保存到账户' : 'Save to account'}
              </button>
              <label
                style={{ ...selectStyle, cursor: canCustom ? 'pointer' : 'not-allowed', display: 'inline-flex', alignItems: 'center', gap: 6, opacity: canCustom ? 1 : 0.5 }}
              >
                {zh ? '上传 .css 文件' : 'Upload .css'}
                <input
                  type="file"
                  accept=".css,text/css"
                  disabled={!canCustom}
                  style={{ display: 'none' }}
                  onChange={(e) => {
                    const file = e.target.files?.[0];
                    if (!file || !canCustom) return;
                    if (file.size > 64 * 1024) { setSaveErr(zh ? '超过 64KB 上限' : 'Exceeds 64KB'); return; }
                    const reader = new FileReader();
                    reader.onload = () => { setCssDraft(String(reader.result || '')); setSaveMsg(''); };
                    reader.readAsText(file);
                    e.target.value = '';
                  }}
                />
              </label>
              {saveMsg && <span style={{ color: 'var(--success-green)', fontSize: 12 }}>{saveMsg}</span>}
              {saveErr && <span style={{ color: 'var(--warning-orange)', fontSize: 12 }}>{saveErr}</span>}
            </div>
          </section>
        )}
        <section aria-label={zh ? '显示设置' : 'Display settings'} style={{ ...panel, marginTop: 28 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', paddingBottom: 12 }}>
            <span style={mono}>{zh ? '显示设置' : 'DISPLAY'}</span>
            <button
              type="button"
              onClick={() => { const id = resetThemePrefs(); setCurrent(id); setPrefs(readThemePrefs()); }}
              style={{ ...selectStyle, cursor: 'pointer', fontSize: 11 }}
            >
              {zh ? '恢复默认' : 'Reset to default'}
            </button>
          </div>

          <Row
            title={zh ? '跟随系统' : 'Follow system'}
            hint={zh ? '按系统的浅色/深色外观自动切换。手动点选下方任一主题会关闭跟随。' : 'Switch automatically with the system light/dark appearance. Picking a theme below turns this off.'}
            control={<Switch checked={prefs.followSystem} onChange={(v) => update({ followSystem: v })} label={zh ? '跟随系统' : 'Follow system'} />}
          >
            {prefs.followSystem && (
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 16, marginTop: 12 }}>
                <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8, fontSize: 12.5 }}>
                  {zh ? '浅色时' : 'When light'}
                  <select value={prefs.light} onChange={(e) => update({ light: e.target.value as ThemeId })} style={selectStyle}>
                    {lights.map((t) => <option key={t.id} value={t.id}>{themeName(t, lang)}</option>)}
                  </select>
                </label>
                <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8, fontSize: 12.5 }}>
                  {zh ? '深色时' : 'When dark'}
                  <select value={prefs.dark} onChange={(e) => update({ dark: e.target.value as ThemeId })} style={selectStyle}>
                    {darks.map((t) => <option key={t.id} value={t.id}>{themeName(t, lang)}</option>)}
                  </select>
                </label>
              </div>
            )}
          </Row>

          <Row
            title={zh ? '星海神经动态网络背景' : 'Neural starfield background'}
            hint={zh ? '背景颜色随主题变化。关闭后动画完全停止，更省电；系统开启"减少动态效果"时只显示静态画面。' : 'Colours follow the theme. Turning it off stops the animation entirely to save power; with reduced motion on, a still frame is shown.'}
            control={<Switch checked={prefs.bgEnabled} onChange={(v) => update({ bgEnabled: v })} label={zh ? '星海神经动态网络背景' : 'Neural starfield background'} />}
          >
            <label style={{ display: 'flex', alignItems: 'center', gap: 12, marginTop: 12, fontSize: 12.5, opacity: prefs.bgEnabled ? 1 : 0.45 }}>
              <span style={{ minWidth: 56 }}>{zh ? '强度' : 'Intensity'}</span>
              <input
                type="range"
                min={0}
                max={100}
                step={5}
                value={prefs.bgIntensity}
                disabled={!prefs.bgEnabled}
                aria-label={zh ? '背景强度' : 'Background intensity'}
                onChange={(e) => update({ bgIntensity: Number(e.target.value) })}
                style={{ flex: 1, maxWidth: 320, accentColor: 'var(--accent-cyan)' }}
              />
              <span style={{ ...mono, minWidth: 36, textAlign: 'right' }}>{prefs.bgIntensity}%</span>
            </label>
          </Row>
        </section>

        {groups.map((group) => (
          <section key={group.family} aria-label={group.title} style={{ marginTop: 36 }}>
            <div style={{ display: 'flex', alignItems: 'baseline', gap: 12, flexWrap: 'wrap' }}>
              <h2 style={{ fontFamily: 'var(--font-display)', fontSize: 20, margin: 0 }}>{group.title}</h2>
              <span style={{ color: 'var(--text-secondary)', fontSize: 13 }}>{group.hint}</span>
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(250px, 1fr))', gap: 16, marginTop: 14 }}>
              {THEMES.filter((t) => themeFamily(t) === group.family).map((theme) => {
                const active = theme.id === current;
                return (
                  <button
                    key={theme.id}
                    type="button"
                    onClick={() => pick(theme.id)}
                    aria-pressed={active}
                    style={{
                      textAlign: 'left',
                      background: 'var(--bg-card-solid)',
                      border: active ? '1px solid var(--accent-cyan)' : '1px solid var(--border-light)',
                      boxShadow: active ? '0 0 0 1px var(--accent-cyan)' : 'none',
                      borderRadius: 16,
                      padding: 16,
                      cursor: 'pointer',
                      color: 'inherit',
                    }}
                  >
                    <MiniPreview theme={theme} />
                    <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8 }}>
                      <strong style={{ fontFamily: 'var(--font-display)' }}>{themeName(theme, lang)}</strong>
                      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)' }}>{zh ? theme.nameEn : theme.name}</span>
                    </div>
                    <p style={{ color: 'var(--text-secondary)', fontSize: 13, lineHeight: 1.6, marginTop: 8 }}>{themeSummary(theme, lang)}</p>
                    <div style={{ display: 'flex', gap: 10, marginTop: 12 }}>
                      <span style={{ ...mono, color: active ? 'var(--accent-cyan)' : 'var(--text-tertiary)' }}>
                        {active ? (zh ? '当前' : 'CURRENT') : (theme.mode === 'dark' ? (zh ? '深色' : 'DARK') : (zh ? '浅色' : 'LIGHT'))}
                      </span>
                      {prefs.followSystem && (prefs.light === theme.id || prefs.dark === theme.id) && (
                        <span style={mono}>{zh ? '· 跟随系统' : '· SYSTEM'}</span>
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          </section>
        ))}
      </main>
    </Shell>
  );
}
