import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import vm from "node:vm";
import { describe, expect, it } from "vitest";
import {
  DEFAULT_THEME_PREFS,
  THEMES,
  parseThemePrefs,
  resolveTheme,
  type ThemeId,
} from "../themes";
import { FALLBACK_SERIES, readChartTheme } from "../chartTheme";

const css = readFileSync(new URL("../../index.css", import.meta.url), "utf8");
const html = readFileSync(
  new URL("../../../index.html", import.meta.url),
  "utf8"
);

/* ---------- 解析 index.css 的主题变量(:root = synapse) ---------- */
function themeVars(): Record<string, Record<string, string>> {
  const out: Record<string, Record<string, string>> = {};
  const re = /(:root|html\[data-theme="([a-z-]+)"\])\s*\{([^}]*)\}/g;
  for (const m of css.matchAll(re)) {
    const id = m[2] ?? "synapse";
    out[id] = {
      ...(out[id] ?? {}),
      ...Object.fromEntries(
        [...m[3].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map(x => [
          x[1],
          x[2].trim(),
        ])
      ),
    };
  }
  return out;
}
const vars = themeVars();
const get = (id: string, name: string): string => {
  let v = vars[id]?.[name] ?? vars.synapse[name];
  for (let i = 0; i < 5 && v?.startsWith("var("); i++) {
    const k = v.slice(4, -1).split(",")[0].trim();
    v = vars[id]?.[k] ?? vars.synapse[k];
  }
  return v;
};
/** "#rrggbb" / "rgba(r, g, b, a)" → [r, g, b, a] */
const parse = (c: string): number[] => {
  if (c.startsWith("#")) {
    const x = c.slice(1);
    return [0, 2, 4].map(i => parseInt(x.slice(i, i + 2), 16)).concat(1);
  }
  const n = c.match(/[\d.]+/g)!.map(Number);
  return [n[0], n[1], n[2], n[3] ?? 1];
};
const over = (fg: number[], bg: number[]) =>
  [0, 1, 2].map(i => fg[i] * fg[3] + bg[i] * (1 - fg[3]));
const hex = (c: number[]) =>
  "#" +
  c
    .slice(0, 3)
    .map(x => Math.round(x).toString(16).padStart(2, "0"))
    .join("");
const rgb = (h: string) => parse(h).slice(0, 3);
/** 文字实际可能落在的底:页面底、实色卡片、最深底,以及半透明卡片叠在最深底上的合成色 */
const surfaces = (id: string) => {
  const voidBg = parse(get(id, "--bg-void"));
  return [
    get(id, "--bg-primary"),
    get(id, "--bg-card-solid"),
    get(id, "--bg-void"),
    hex(over(parse(get(id, "--bg-card")), voidBg)),
  ];
};
const lum = (h: string) => {
  const [r, g, b] = rgb(h).map(c => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a: string, b: string) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

describe("theme tokens: contrast", () => {
  for (const theme of THEMES) {
    it(`${theme.id}: tertiary text meets WCAG AA (4.5:1) on page and card surfaces`, () => {
      for (const bg of surfaces(theme.id)) {
        expect(
          contrast(get(theme.id, "--text-tertiary"), bg)
        ).toBeGreaterThanOrEqual(4.5);
      }
    });
    it(`${theme.id}: accent text tokens (cyan / purple) meet WCAG AA`, () => {
      for (const fg of ["--accent-cyan", "--accent-purple"]) {
        for (const bg of surfaces(theme.id)) {
          expect(contrast(get(theme.id, fg), bg)).toBeGreaterThanOrEqual(4.5);
        }
      }
    });
    it(`${theme.id}: primary-button label (--on-accent) meets WCAG AA on its fill and hover fill`, () => {
      const fills = theme.id.startsWith("x-")
        ? ["--accent-solid", "--accent-solid-hover"]
        : ["--accent-cyan", "--accent-cyan-bright"];
      for (const bg of fills)
        expect(
          contrast(get(theme.id, "--on-accent"), get(theme.id, bg))
        ).toBeGreaterThanOrEqual(4.5);
    });
    it(`${theme.id}: preview swatch accent matches the CSS accent token`, () => {
      const accent = theme.id.startsWith("x-")
        ? null
        : get(theme.id, "--accent-cyan");
      if (accent)
        expect(theme.swatches[3].toLowerCase()).toBe(accent.toLowerCase());
    });
    it(`${theme.id}: 8 distinct chart series, each ≥3:1 against the card surface (WCAG 1.4.11)`, () => {
      const series = [1, 2, 3, 4, 5, 6, 7, 8].map(i =>
        get(theme.id, `--chart-${i}`)
      );
      expect(series.every(c => /^#[0-9a-fA-F]{6}$/.test(c))).toBe(true);
      expect(new Set(series.map(c => c.toLowerCase())).size).toBe(8);
      for (const c of series)
        for (const bg of surfaces(theme.id))
          expect(contrast(c, bg)).toBeGreaterThanOrEqual(3);
    });
  }
  it("derives chart chrome (grid / tooltip) from surface tokens so every theme adapts", () => {
    expect(vars.synapse["--chart-grid"]).toBe("var(--border-light)");
    expect(vars.synapse["--chart-tooltip-bg"]).toBe("var(--bg-card-solid)");
    expect(vars.synapse["--chart-tooltip-text"]).toBe("var(--text-primary)");
  });
  it("falls back to the Synapse palette outside the browser", () => {
    expect(readChartTheme().series).toEqual(FALLBACK_SERIES);
    expect(FALLBACK_SERIES).toEqual(
      [1, 2, 3, 4, 5, 6, 7, 8].map(i => get("synapse", `--chart-${i}`))
    );
  });
});

/* ---------- 语义 token 完整性:被引用的变量在每个主题都有值 ---------- */
const TEXT_TOKENS = [
  "--text-secondary",
  "--success-green",
  "--danger-red",
  "--warning-orange",
  "--accent-gold",
  "--accent-magenta",
  "--accent-orange",
  "--accent-warning",
  "--accent-lime",
];
describe("theme tokens: semantic text colours", () => {
  for (const theme of THEMES) {
    it(`${theme.id}: every semantic text token meets WCAG AA on page and card`, () => {
      for (const fg of TEXT_TOKENS)
        for (const bg of surfaces(theme.id)) {
          const c = contrast(get(theme.id, fg), bg);
          expect(c, `${fg} on ${bg} = ${c.toFixed(2)}`).toBeGreaterThanOrEqual(
            4.5
          );
        }
    });
    it(`${theme.id}: warning button label (--on-accent-orange) meets WCAG AA`, () => {
      expect(
        contrast(
          get(theme.id, "--on-accent-orange"),
          get(theme.id, "--accent-orange")
        )
      ).toBeGreaterThanOrEqual(4.5);
    });
  }
});

const SRC = new URL("../../", import.meta.url).pathname;
function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap(e =>
    e.isDirectory()
      ? e.name === "__tests__"
        ? []
        : walk(join(dir, e.name))
      : /\.(tsx?|css)$/.test(e.name)
        ? [join(dir, e.name)]
        : []
  );
}
/** 运行时由 JS 写入的变量(applyBackgroundPrefs 等) */
const RUNTIME_VARS = new Set(["--nn-intensity"]);
describe("theme tokens: no undefined CSS variables", () => {
  // 去掉只在 X 家族作用域内生效的规则(那些规则里引用 X 专用 token 是合法的)
  const scoped = (src: string) =>
    src.replace(/html\[data-theme\^="x-"\][^{]*\{[^}]*\}/g, "");
  const refs = new Map<string, string>();
  for (const f of walk(SRC)) {
    const text = f.endsWith(".css")
      ? scoped(readFileSync(f, "utf8"))
      : readFileSync(f, "utf8");
    // 只检查无回退值的 var(--x)
    for (const m of text.matchAll(/var\((--[\w-]+)\s*\)/g))
      if (!RUNTIME_VARS.has(m[1])) refs.set(m[1], f.slice(SRC.length));
  }
  // 组件局部变量(如 .observatory-frame 的 --tick):在非主题规则里声明的,按局部作用域放行
  const local = new Set(
    [
      ...css
        .replace(/(:root|html\[data-theme[^\]]*\])[^{]*\{[^}]*\}/g, "")
        .matchAll(/(--[\w-]+)\s*:/g),
    ].map(m => m[1])
  );
  for (const theme of THEMES) {
    it(`${theme.id}: every var() referenced without a fallback resolves`, () => {
      const missing = [...refs].filter(
        ([name]) =>
          !local.has(name) &&
          vars[theme.id]?.[name] === undefined &&
          vars.synapse[name] === undefined
      );
      expect(missing).toEqual([]);
    });
  }
});

/* ---------- index.html 无闪烁启动脚本 ---------- */
const bootSrc = (() => {
  const start = html.indexOf("<script>\n      (function () {");
  return html.slice(
    start + "<script>".length,
    html.indexOf("</script>", start)
  );
})();

function runBoot(ls: Record<string, string>, systemDark: boolean | null) {
  const meta = {
    content: "#07070a",
    setAttribute(_: string, v: string) {
      this.content = v;
    },
  };
  const props: Record<string, string> = {};
  const root = {
    dataset: {} as Record<string, string>,
    style: {
      colorScheme: "",
      setProperty: (k: string, v: string) => {
        props[k] = v;
      },
    },
  };
  const ctx = {
    localStorage: { getItem: (k: string) => (k in ls ? ls[k] : null) },
    window:
      systemDark === null
        ? {}
        : { matchMedia: () => ({ matches: systemDark }) },
    document: { documentElement: root, querySelector: () => meta },
    JSON,
    Math,
    String,
    isFinite,
  };
  vm.runInNewContext(bootSrc, ctx);
  return {
    theme: root.dataset.theme,
    scheme: root.style.colorScheme,
    meta: meta.content,
    nnBg: root.dataset.nnBg,
    props,
  };
}

describe("no-flash boot script (index.html)", () => {
  it("keeps its theme map in sync with the registry", () => {
    const map = JSON.parse(bootSrc.match(/var M = (\{.*?\});/)![1]) as Record<
      string,
      [string, string]
    >;
    expect(Object.keys(map)).toEqual(THEMES.map(t => t.id));
    for (const t of THEMES) expect(map[t.id]).toEqual([t.mode, t.swatches[0]]);
    expect(bootSrc).toContain(`'${DEFAULT_THEME_PREFS.dark}'`);
    expect(bootSrc).toContain(`'${DEFAULT_THEME_PREFS.light}'`);
  });

  const cases: Array<[string, Record<string, string>, boolean | null]> = [
    ["nothing stored", {}, true],
    ["manual paper", { "epicode-theme": "paper" }, true],
    ["invalid stored id", { "epicode-theme": "nope" }, false],
    [
      "follow system, light",
      { "epicode-theme-prefs": JSON.stringify({ followSystem: true }) },
      false,
    ],
    [
      "follow system, dark, custom slots",
      {
        "epicode-theme": "paper",
        "epicode-theme-prefs": JSON.stringify({
          followSystem: true,
          light: "daylight",
          dark: "amber",
        }),
      },
      true,
    ],
    [
      "follow system, wrong-mode slot",
      {
        "epicode-theme-prefs": JSON.stringify({
          followSystem: true,
          light: "x-dark",
        }),
      },
      false,
    ],
    [
      "follow system, no matchMedia",
      { "epicode-theme-prefs": JSON.stringify({ followSystem: true }) },
      null,
    ],
    [
      "malformed prefs",
      { "epicode-theme": "amber", "epicode-theme-prefs": "{oops" },
      false,
    ],
  ];
  for (const [name, ls, systemDark] of cases) {
    it(`matches runtime resolveTheme: ${name}`, () => {
      const prefs = parseThemePrefs(ls["epicode-theme-prefs"] ?? null);
      const stored = (
        THEMES.some(t => t.id === ls["epicode-theme"])
          ? ls["epicode-theme"]
          : "synapse"
      ) as ThemeId;
      const expected = resolveTheme(prefs, stored, systemDark ?? true);
      const spec = THEMES.find(t => t.id === expected)!;
      const out = runBoot(ls, systemDark);
      expect(out.theme).toBe(expected);
      expect(out.scheme).toBe(spec.mode);
      expect(out.meta).toBe(spec.swatches[0]);
      expect(out.nnBg).toBe(prefs.bgEnabled ? "on" : "off");
    });
  }

  it("applies background prefs before first paint", () => {
    const out = runBoot(
      {
        "epicode-theme-prefs": JSON.stringify({
          bgEnabled: false,
          bgIntensity: 40,
        }),
      },
      true
    );
    expect(out.nnBg).toBe("off");
    expect(out.props["--nn-intensity"]).toBe("0.4");
  });
});
