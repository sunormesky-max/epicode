/**
 * 构建时镜像生成 — 从 Benchmarks.tsx 单一事实源自动重写 benchmarks.md 的数据段
 * 手写散文保留, 数据块由 AUTO 标记圈定, 每次构建自动刷新。治镜像漂移。
 * 解析: 字面量 → JSON 变换后 JSON.parse(无 eval)。
 */
import { readFileSync, writeFileSync } from 'node:fs';

const SRC = 'src/pages/Benchmarks.tsx';
const MD = 'public/benchmarks.md';

function literalToJson(lit) {
  return lit
    .replace(/\/\/[^\n]*/g, '')            // 去行注释
    .replace(/'/g, '"')                     // 单引号→双引号(字面量内无转义单引号)
    .replace(/([{,]\s*)(\w+)\s*:/g, '$1"$2":') // 裸键→带引号键
    .replace(/,(\s*[\]}])/g, '$1');         // 去尾逗号
}

function extractConst(name) {
  const src = readFileSync(SRC, 'utf-8');
  const re = new RegExp(`const ${name}[^=]*=\\s*(\\[[\\s\\S]*?\\n\\]);`, 'm');
  const m = src.match(re);
  if (!m) throw new Error(`const ${name} not found in ${SRC}`);
  return JSON.parse(literalToJson(m[1]));
}

const LME = extractConst('LME_OVERALL');
const BY = extractConst('LME_BY_TYPE');
const MODE = extractConst('MODE_BENCH_0819');
const KG = extractConst('KG_HEALTH_0819');
const SMRP = extractConst('SMRP_FRESH_0819');

const lines = [];
lines.push(`## LongMemEval-S oracle, 500 questions (overall, auto-generated ${new Date().toISOString().slice(0, 10)})`);
lines.push('| mode | score |');
lines.push('|---|---|');
for (const r of LME) lines.push(`| ${r.mode} | ${r.score}% |`);
lines.push('');
lines.push('## By question type (n / hybrid / semantic / ppr)');
for (const r of BY) lines.push(`- ${r.type}: ${r.n} / ${r.hybrid} / ${r.semantic} / ${r.ppr}`);
lines.push('');
lines.push('## Search latency (P50 ms / avg results)');
for (const r of MODE) lines.push(`- ${r.mode}: ${r.p50} / ${r.avg_results}`);
lines.push('');
lines.push('## Knowledge-graph health');
for (const r of KG) lines.push(`- ${r.k}: ${r.v}`);
lines.push('');
lines.push('## SMRP operation latency (ms; fresh loopback vs old public)');
for (const r of SMRP) lines.push(`- ${r.op}: ${r.fresh}${r.old != null ? ` (was ${r.old})` : ' (n/a)'}`);
const fmt = lines.join('\n');

const md = readFileSync(MD, 'utf-8');
const OPEN = '<!-- AUTO:BENCH -->', CLOSE = '<!-- /AUTO:BENCH -->';
if (!md.includes(OPEN)) throw new Error(`${MD}: AUTO markers missing — wrap the data sections with ${OPEN} / ${CLOSE}`);
const esc = (s) => s.replace(/[!*]/g, '\\$&');
const next = md.replace(new RegExp(`${esc(OPEN)}[\\s\\S]*?${esc(CLOSE)}`), `${OPEN}\n${fmt}\n${CLOSE}`);
if (next !== md) { writeFileSync(MD, next); console.log('[gen-mirrors] benchmarks.md data refreshed'); }
else console.log('[gen-mirrors] benchmarks.md already current');
