import { useMemo, useSyncExternalStore } from "react";
import { subscribeTheme, themeKey } from "./chartTheme";

/**
 * 知识图谱 canvas 调色板。
 *
 * canvas 的 fillStyle / strokeStyle 不能解析 `var()`,且绘制代码大量使用 `hex + "40"`
 * 这种十六进制透明度拼接,所以这里在主题切换时把 CSS 变量解析为小写 `#rrggbb`。
 * 回落值 = 突触青默认主题的原硬编码值,保证默认主题像素不变。
 */
export interface GraphPalette {
  /** 聚类色,按 `cluster % length` 取用 */
  cluster: string[];
  /** 关系类型 → 颜色 */
  edge: Record<string, string>;
  accent: string;
  purple: string;
  gold: string;
  muted: string;
  path: string;
  /** 层级底色用:神经第三色 */
  tertiary: string;
  /** 标签底 pill 的 "r, g, b" 三元组 */
  labelBg: string;
  labelText: string;
}

type Slot = "accent" | "purple" | 1 | 2 | 3 | 4 | 5 | 6;
/** 原 CLUSTER_COLORS 的结构:以主色为主,辅以紫与 6 个点缀色 */
const CLUSTER_SLOTS: Slot[] = [
  "accent",
  "accent",
  "accent",
  1,
  "accent",
  "purple",
  "purple",
  2,
  "purple",
  "accent",
  "accent",
  3,
  4,
  5,
  6,
];

export const FALLBACK_GRAPH = {
  accent: "#3ecfae",
  purple: "#8b7ec8",
  crimson: "#ff3860",
  gold: "#e6c878",
  tertiary: "#5a9a8c",
  muted: "#6b7280",
  path: "#ffd700",
  labelBg: "8, 10, 18",
  labelText: "#f0f0f5",
  accents: ["#7ba3ff", "#ec4899", "#22d3ee", "#818cf8", "#e879f9", "#facc15"],
};

/** "#abc" / "#AABBCC" / "rgb(1, 2, 3)" → "#aabbcc";无法解析时返回 fallback。 */
export function toHex(value: string | undefined, fallback: string): string {
  const v = (value ?? "").trim().toLowerCase();
  let m = /^#([0-9a-f]{6})$/.exec(v);
  if (m) return `#${m[1]}`;
  m = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/.exec(v);
  if (m) return `#${m[1]}${m[1]}${m[2]}${m[2]}${m[3]}${m[3]}`;
  m =
    /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*1(?:\.0+)?\s*)?\)$/.exec(
      v
    );
  if (m)
    return `#${[m[1], m[2], m[3]].map(x => Number(x).toString(16).padStart(2, "0")).join("")}`;
  return fallback;
}

/** "#rrggbb" + alpha → "rgba(r,g,b,a)" */
export function withAlpha(hex: string, alpha: number): string {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${alpha})`;
}

export function readGraphPalette(root?: HTMLElement): GraphPalette {
  const el =
    root ??
    (typeof document !== "undefined" ? document.documentElement : undefined);
  const cs =
    el && typeof getComputedStyle === "function"
      ? getComputedStyle(el)
      : undefined;
  const raw = (name: string) => cs?.getPropertyValue(name).trim() ?? "";
  const hex = (name: string, fb: string) => toHex(raw(name), fb);
  const F = FALLBACK_GRAPH;
  const accent = hex("--accent-cyan", F.accent);
  const purple = hex("--accent-purple", F.purple);
  const accents = F.accents.map((fb, i) => hex(`--graph-accent-${i + 1}`, fb));
  const cluster = CLUSTER_SLOTS.map(s =>
    s === "accent" ? accent : s === "purple" ? purple : accents[s - 1]
  );
  const crimson = hex("--accent-crimson", F.crimson);
  const labelBg = raw("--graph-label-bg-rgb");
  return {
    cluster,
    edge: {
      similar: accent,
      related: accent,
      contradicts: crimson,
      precedes: accent,
      contains: accent,
    },
    accent,
    purple,
    gold: hex("--accent-gold", F.gold),
    muted: hex("--graph-muted", F.muted),
    path: hex("--graph-path", F.path),
    tertiary: hex("--neural-tertiary", F.tertiary),
    labelBg: /^\d+\s*,\s*\d+\s*,\s*\d+$/.test(labelBg) ? labelBg : F.labelBg,
    labelText: hex("--graph-label-text", F.labelText),
  };
}

/** 订阅主题切换,返回解析后的图谱调色板(引用在主题不变时保持稳定)。 */
export function useGraphPalette(): GraphPalette {
  const key = useSyncExternalStore(subscribeTheme, themeKey, () => "");
  // eslint-disable-next-line react-hooks/exhaustive-deps -- key 变化即主题变化,需要重新解析
  return useMemo(() => readGraphPalette(), [key]);
}
