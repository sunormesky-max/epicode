// 构建后守卫:入口 HTML 只允许预加载 React 运行时,禁止把图表 / 动画等路由级依赖拉进首屏。
// 回归场景:vite.config.ts 的 manualChunks 若再把 react/jsx-runtime 等共享模块分进
// 某个重型 vendor 块,入口就会静态 import 它,所有页面(含登录页)都要多下 100+ KB。
import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';

const dist = process.env.DIST_DIR ? resolve(process.env.DIST_DIR) : resolve(import.meta.dirname, '../dist/public');
const html = readFileSync(resolve(dist, 'index.html'), 'utf8');
const entry = [...html.matchAll(/<script[^>]+type="module"[^>]+src="\.\/assets\/([^"]+)"/g)].map((m) => m[1]);
const preloads = [...html.matchAll(/<link[^>]+rel="modulepreload"[^>]+href="\.\/assets\/([^"]+)"/g)].map((m) => m[1]);
const FORBIDDEN = [
  ['recharts', /recharts|Recharts|ResponsiveContainer/],
  ['framer-motion', /framer-motion|MotionValue|useReducedMotion|AnimatePresence/],
];
const problems = [];
for (const file of [...entry, ...preloads]) {
  const src = readFileSync(resolve(dist, 'assets', file), 'utf8');
  for (const [name, re] of FORBIDDEN) if (re.test(src)) problems.push(`${file} (首屏) 含 ${name}`);
}
const all = readdirSync(resolve(dist, 'assets')).filter((f) => f.endsWith('.js'));
if (!all.length || !entry.length) problems.push('dist 中未找到入口脚本');
if (problems.length) {
  console.error('[check-entry-chunks] 首屏块包含路由级依赖:\n  ' + problems.join('\n  '));
  process.exit(1);
}
console.log(`[check-entry-chunks] ok: entry=${entry.join(',')} preload=${preloads.join(',') || '(none)'}`);
