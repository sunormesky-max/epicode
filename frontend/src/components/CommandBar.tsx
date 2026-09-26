import { useEffect, useMemo, useRef, useState, useCallback } from 'react';
import { useNavigate, useLocation } from 'react-router';
import {
  LayoutDashboard, Brain, GitBranch, Wrench, Users, Archive, Activity, MessageSquare,
  Home, BookOpen, Compass, UsersRound, BarChart3, Network, Sparkles,
  LogOut, Copy, CornerDownLeft, Search, Radio,
} from 'lucide-react';
import { getApiKey, logout } from '@/lib/api';

/**
 * 命令条 — ⌘K / Ctrl+K
 * 观测站的操作方式:菜单降级为命令,键盘优先。
 */

interface Cmd {
  id: string;
  label: string;
  hint?: string;
  group: 'NAVIGATE' | 'PUBLIC' | 'ACTION';
  icon: React.ComponentType<{ size?: number | string; style?: React.CSSProperties }>;
  run: () => void;
}

export default function CommandBar() {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState('');
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const navigate = useNavigate();
  const location = useLocation();

  const go = useCallback((hash: string) => { window.location.hash = hash; setOpen(false); }, []);

  const commands: Cmd[] = useMemo(() => {
    const nav: Cmd[] = [
      { id: 'overview', label: 'Overview', hint: '概览', group: 'NAVIGATE', icon: LayoutDashboard, run: () => go('#/dashboard') },
      { id: 'memories', label: 'Memories', hint: '记忆', group: 'NAVIGATE', icon: Brain, run: () => go('#/dashboard/memories') },
      { id: 'chat', label: 'Chat', hint: '对话', group: 'NAVIGATE', icon: MessageSquare, run: () => go('#/dashboard/chat') },
      { id: 'graph', label: 'Graph', hint: '图谱', group: 'NAVIGATE', icon: GitBranch, run: () => go('#/dashboard/graph') },
      { id: 'archive', label: 'Archive', hint: '归档', group: 'NAVIGATE', icon: Archive, run: () => go('#/dashboard/archive') },
      { id: 'cognitive', label: 'Cognitive Engine', hint: '认知引擎', group: 'NAVIGATE', icon: Activity, run: () => go('#/dashboard/cognitive') },
      { id: 'observe', label: 'Observation Deck', hint: '观测舱', group: 'NAVIGATE', icon: Radio, run: () => go('#/dashboard/observe') },
      { id: 'skills', label: 'Skills', hint: '技能', group: 'NAVIGATE', icon: Wrench, run: () => go('#/dashboard/skills') },
      { id: 'accounts', label: 'Sub Accounts', hint: '子账号', group: 'NAVIGATE', icon: Users, run: () => go('#/dashboard/accounts') },
    ];
    const pub: Cmd[] = [
      { id: 'home', label: 'Home', hint: '首页 · 下潜场域', group: 'PUBLIC', icon: Home, run: () => go('#/') },
      { id: 'docs', label: 'Docs', hint: '文档', group: 'PUBLIC', icon: BookOpen, run: () => go('#/docs') },
      { id: 'guide', label: 'Guide', hint: '快速上手', group: 'PUBLIC', icon: Compass, run: () => go('#/guide') },
      { id: 'benchmarks', label: 'Benchmarks', hint: '评测基准', group: 'PUBLIC', icon: BarChart3, run: () => go('#/benchmarks') },
      { id: 'smrp', label: 'SMRP Protocol', hint: '技能市场协议', group: 'PUBLIC', icon: Network, run: () => go('#/smrp') },
      { id: 'l0', label: 'L0 Protocol', hint: '主动推理', group: 'PUBLIC', icon: Sparkles, run: () => go('#/l0') },
      { id: 'community', label: 'Community', hint: '社区', group: 'PUBLIC', icon: UsersRound, run: () => go('#/community') },
    ];
    const actions: Cmd[] = [
      {
        id: 'copy-key', label: 'Copy API Key', hint: '复制密钥', group: 'ACTION', icon: Copy,
        run: () => { const k = getApiKey(); if (k) navigator.clipboard.writeText(k); setOpen(false); },
      },
      {
        id: 'logout', label: 'Log Out', hint: '退出', group: 'ACTION', icon: LogOut,
        run: () => { logout(); window.location.hash = '#/'; setOpen(false); },
      },
    ];
    return [...nav, ...pub, ...actions];
  }, [go]);

  const filtered = useMemo(() => {
    const s = q.trim().toLowerCase();
    if (!s) return commands;
    const hits = commands.filter(c =>
      c.label.toLowerCase().includes(s) || (c.hint ?? '').includes(s) || c.group.toLowerCase().includes(s)
    );
    // 叛逆者 R7: 自由文本 → "问场" — 命令条即神谕入口
    if (hits.length === 0) {
      return [{
        id: 'ask-field', label: `Ask Field: "${q.trim()}"`, hint: '带着问题进入对话', group: 'ACTION' as const,
        icon: Search, run: () => { window.location.hash = `#/dashboard/chat?q=${encodeURIComponent(q.trim())}`; setOpen(false); },
      }];
    }
    return hits;
  }, [q, commands]);

  // 全局快捷键: ⌘K / Ctrl+K
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setOpen(o => { setQ(''); setSel(0); return !o; });
      }
      if (e.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // 打开时聚焦
  useEffect(() => {
    if (open) requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  // 选中项滚动可见
  useEffect(() => {
    listRef.current?.querySelector(`[data-idx="${sel}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [sel]);

  useEffect(() => { setSel(0); }, [q]);

  if (!open) {
    return (
      <button
        onClick={() => setOpen(true)}
        aria-label="open command bar"
        className="fixed bottom-5 right-5 z-40 flex items-center gap-2 px-3 py-2 rounded-lg transition-all"
        style={{
          background: 'rgba(16, 16, 24, 0.85)', backdropFilter: 'blur(12px)',
          border: '1px solid var(--border-medium)', color: 'var(--text-tertiary)',
          fontFamily: 'var(--font-mono)', fontSize: 12,
        }}
      >
        <Search size={13} />
        <span className="hidden sm:inline">⌘K</span>
      </button>
    );
  }

  const onInputKey = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setSel(s => Math.min(s + 1, filtered.length - 1)); }
    if (e.key === 'ArrowUp') { e.preventDefault(); setSel(s => Math.max(s - 1, 0)); }
    if (e.key === 'Enter') { e.preventDefault(); filtered[sel]?.run(); }
  };

  let lastGroup = '';

  return (
    <div className="fixed inset-0 z-[70] flex items-start justify-center pt-[14vh] px-4" style={{ background: 'rgba(7,7,10,0.6)', backdropFilter: 'blur(4px)' }} onClick={() => setOpen(false)}>
      <div
        className="w-full max-w-lg rounded-2xl overflow-hidden"
        style={{ background: 'rgba(16,16,24,0.97)', border: '1px solid var(--border-medium)', boxShadow: '0 24px 80px rgba(0,0,0,0.6)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 px-4" style={{ borderBottom: '1px solid var(--border-light)' }}>
          <Search size={16} style={{ color: 'var(--accent-cyan)', flexShrink: 0 }} />
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={onInputKey}
            placeholder="Navigate or act…"
            className="w-full py-4 bg-transparent outline-none"
            style={{ color: 'var(--text-primary)', fontFamily: 'var(--font-body)', fontSize: 15 }}
          />
          <kbd style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', border: '1px solid var(--border-light)', borderRadius: 4, padding: '2px 6px' }}>ESC</kbd>
        </div>

        <div ref={listRef} className="max-h-[46vh] overflow-y-auto py-2">
          {filtered.length === 0 && (
            <div className="px-4 py-6 text-sm" style={{ color: 'var(--text-tertiary)' }}>no match</div>
          )}
          {filtered.map((c, i) => {
            const showGroup = c.group !== lastGroup;
            lastGroup = c.group;
            const on = i === sel;
            return (
              <div key={c.id}>
                {showGroup && (
                  <div className="px-4 pt-3 pb-1" style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.14em' }}>
                    {c.group}
                  </div>
                )}
                <button
                  data-idx={i}
                  onClick={() => c.run()}
                  onMouseEnter={() => setSel(i)}
                  className="w-full flex items-center gap-3 px-4 py-2.5 text-left transition-colors"
                  style={{ background: on ? 'rgba(62,207,174,0.07)' : 'transparent', borderLeft: on ? '2px solid var(--accent-cyan)' : '2px solid transparent', cursor: 'pointer' }}
                >
                  <c.icon size={15} style={{ color: on ? 'var(--accent-cyan)' : 'var(--text-tertiary)', flexShrink: 0 }} />
                  <span style={{ color: 'var(--text-primary)', fontSize: 14.5, flex: 1 }}>{c.label}</span>
                  {c.hint && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-tertiary)' }}>{c.hint}</span>}
                  {on && <CornerDownLeft size={12} style={{ color: 'var(--accent-cyan)' }} />}
                </button>
              </div>
            );
          })}
        </div>

        <div className="flex items-center gap-4 px-4 py-2.5" style={{ borderTop: '1px solid var(--border-light)', fontFamily: 'var(--font-mono)', fontSize: 10.5, color: 'var(--text-tertiary)' }}>
          <span>↑↓ navigate</span><span>↵ run</span><span style={{ marginLeft: 'auto' }}>{location.pathname}</span>
        </div>
      </div>
    </div>
  );
}
