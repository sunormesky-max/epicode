import { useMemo, useSyncExternalStore } from "react";
import type { CSSProperties } from "react";

/**
 * 主题感知的图表调色板。
 *
 * recharts / canvas 需要具体颜色值(SVG 呈现属性与 canvas 不能可靠解析 `var()`),
 * 所以这里在主题切换时从 CSS 变量解析出实际颜色:
 * - `--chart-1..8`:系列色(每个主题单独定义,≥3:1 对卡片底,前 4 色两两可区分)
 * - `--chart-grid / --chart-axis / --chart-tooltip-*`:由表面 token 推导,所有主题自动适配
 */
export interface ChartTheme {
  /** 8 个系列色,按序使用;`series[i % 8]` */
  series: string[];
  grid: string;
  axis: string;
  cursor: string;
  tooltip: CSSProperties;
}

/** 解析失败(非浏览器 / 变量缺失)时的回落值 = 突触青默认主题。 */
export const FALLBACK_SERIES = [
  "#3ecfae",
  "#9d8fe0",
  "#e6c878",
  "#f08a8a",
  "#5fb3f0",
  "#a3d977",
  "#e48bc8",
  "#9a9aa6",
];

export function readChartTheme(root?: HTMLElement): ChartTheme {
  const el =
    root ??
    (typeof document !== "undefined" ? document.documentElement : undefined);
  const cs =
    el && typeof getComputedStyle === "function"
      ? getComputedStyle(el)
      : undefined;
  const v = (name: string, fallback: string) =>
    cs?.getPropertyValue(name).trim() || fallback;
  return {
    series: FALLBACK_SERIES.map((fb, i) => v(`--chart-${i + 1}`, fb)),
    grid: v("--chart-grid", "rgba(245, 244, 240, 0.07)"),
    axis: v("--chart-axis", "#7d7d85"),
    cursor: v("--chart-cursor", "rgba(245, 244, 240, 0.07)"),
    tooltip: {
      background: v("--chart-tooltip-bg", "#101018"),
      border: `1px solid ${v("--chart-tooltip-border", "rgba(245, 244, 240, 0.14)")}`,
      color: v("--chart-tooltip-text", "#f5f4f0"),
      borderRadius: 12,
      fontSize: 12,
    },
  };
}

function subscribe(onChange: () => void) {
  if (typeof document === "undefined") return () => {};
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });
  window.addEventListener("epicode-theme", onChange);
  return () => {
    observer.disconnect();
    window.removeEventListener("epicode-theme", onChange);
  };
}

const themeKey = () =>
  typeof document === "undefined"
    ? ""
    : (document.documentElement.dataset.theme ?? "");

/** 订阅 `<html data-theme>`,主题切换时重新解析图表颜色。 */
export function useChartTheme(): ChartTheme {
  const key = useSyncExternalStore(subscribe, themeKey, () => "");
  // key 变化即主题变化:重新从 CSS 变量解析
  // eslint-disable-next-line react-hooks/exhaustive-deps -- key 变化即主题变化,需要重新解析
  return useMemo(() => readChartTheme(), [key]);
}
