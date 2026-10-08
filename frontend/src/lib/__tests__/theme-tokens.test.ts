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
  ACCENTS,
  isAccentId,
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
    accent: root.dataset.accent,
    mode: root.dataset.mode,
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

/* ---------- PR B: RGB 三元组 / 图谱与神经背景 token ---------- */
describe("theme tokens: rgb triplets mirror their hex tokens", () => {
  const PAIRS = [
    "--accent-cyan",
    "--accent-purple",
    "--danger-red",
    "--text-primary",
    "--accent-gold",
    "--bg-card-solid",
  ];
  const triplet = (v: string) => v.split(",").map(x => Number(x.trim()));
  for (const t of THEMES) {
    it(`${t.id}: --*-rgb === hex token`, () => {
      for (const name of PAIRS)
        expect([name, triplet(get(t.id, `${name}-rgb`))]).toEqual([
          name,
          rgb(get(t.id, name).toLowerCase()),
        ]);
    });
    it(`${t.id}: --overlay-rgb lightens dark themes and inks light themes`, () => {
      const want =
        t.mode === "dark"
          ? [255, 255, 255]
          : rgb(get(t.id, "--text-primary").toLowerCase());
      expect(triplet(get(t.id, "--overlay-rgb"))).toEqual(want);
    });
  }
});

describe("theme tokens: canvas palettes (graph + neural background)", () => {
  const HEX = /^#[0-9a-fA-F]{6}$/;
  for (const t of THEMES) {
    it(`${t.id}: --nn-* and --graph-* resolve to concrete colours`, () => {
      for (const n of ["primary", "hi", "secondary", "fade", "gold"])
        expect([n, get(t.id, `--nn-${n}`)]).toEqual([
          n,
          expect.stringMatching(HEX),
        ]);
      for (const n of ["--graph-muted", "--graph-path", "--graph-label-text"])
        expect([n, get(t.id, n)]).toEqual([n, expect.stringMatching(HEX)]);
      expect(get(t.id, "--graph-label-bg-rgb")).toMatch(/^\d+, \d+, \d+$/);
    });
    it(`${t.id}: graph muted text is AA on the canvas background (synapse: pre-existing 4.16, kept for pixel parity)`, () => {
      const c = contrast(get(t.id, "--graph-muted"), get(t.id, "--bg-void"));
      if (t.id === "synapse") expect(c).toBeGreaterThan(4);
      else expect(c).toBeGreaterThanOrEqual(4.5);
    });
    it(`${t.id}: graph node labels are AA on their pill`, () => {
      const pill = hex(
        over(
          [...get(t.id, "--graph-label-bg-rgb").split(",").map(Number), 0.8],
          parse(get(t.id, "--bg-void"))
        )
      );
      expect(
        contrast(get(t.id, "--graph-label-text"), pill)
      ).toBeGreaterThanOrEqual(4.5);
    });
  }
  it("fallback palette equals the synapse (:root) tokens — default theme stays pixel-identical", () => {
    const s = (n: string) => get("synapse", n).toLowerCase();
    expect(FALLBACK_GRAPH.accent).toBe(s("--accent-cyan"));
    expect(FALLBACK_GRAPH.purple).toBe(s("--accent-purple"));
    expect(FALLBACK_GRAPH.crimson).toBe(s("--accent-crimson"));
    expect(FALLBACK_GRAPH.gold).toBe(s("--accent-gold"));
    expect(FALLBACK_GRAPH.tertiary).toBe(s("--neural-tertiary"));
    expect(FALLBACK_GRAPH.muted).toBe(s("--graph-muted"));
    expect(FALLBACK_GRAPH.path).toBe(s("--graph-path"));
    expect(FALLBACK_GRAPH.labelText).toBe(s("--graph-label-text"));
    expect(FALLBACK_GRAPH.labelBg).toBe(s("--graph-label-bg-rgb"));
    expect(FALLBACK_GRAPH.accents).toEqual(
      [1, 2, 3, 4, 5, 6].map(i => s(`--graph-accent-${i}`))
    );
  });
  it("readGraphPalette without a DOM returns the synapse palette", () => {
    const p = readGraphPalette();
    expect(p.cluster).toHaveLength(15);
    expect(p.edge.contradicts).toBe(FALLBACK_GRAPH.crimson);
    expect(p.cluster.slice(0, 4)).toEqual([
      FALLBACK_GRAPH.accent,
      FALLBACK_GRAPH.accent,
      FALLBACK_GRAPH.accent,
      FALLBACK_GRAPH.accents[0],
    ]);
  });
  it("toHex / withAlpha normalise computed-style values", () => {
    expect(toHex("#0F1419", "#000000")).toBe("#0f1419");
    expect(toHex("#abc", "#000000")).toBe("#aabbcc");
    expect(toHex("rgb(29, 155, 240)", "#000000")).toBe("#1d9bf0");
    expect(toHex("", "#123456")).toBe("#123456");
    expect(toHex("var(--x)", "#123456")).toBe("#123456");
    expect(withAlpha("#3ecfae", 0.18)).toBe("rgba(62,207,174,0.18)");
  });
});

/* ---------- PR C: 强调色层 × 每个主题 ---------- */
const accentBlocks = (() => {
  const out: Record<
    string,
    { dark: Record<string, string>; light: Record<string, string> }
  > = {};
  const re =
    /html\[data-accent="([a-z]+)"\](\[data-mode="light"\])?\s*\{([^}]*)\}/g;
  for (const m of css.matchAll(re)) {
    out[m[1]] ??= { dark: {}, light: {} };
    const decl = Object.fromEntries(
      [...m[3].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map(x => [
        x[1],
        x[2].trim(),
      ])
    );
    Object.assign(out[m[1]][m[2] ? "light" : "dark"], decl);
  }
  return out;
})();
/** 主题 × 强调色 下某 token 的生效值(浅色主题:light 块覆盖通用块,与 CSS 特异性一致) */
const getA = (themeId: string, accent: string, name: string): string => {
  if (accent === "theme") return get(themeId, name);
  const mode = THEMES.find(t => t.id === themeId)!.mode;
  const b = accentBlocks[accent];
  const v = (mode === "light" ? b.light[name] : undefined) ?? b.dark[name];
  if (!v) return get(themeId, name);
  return v.startsWith("var(")
    ? get(themeId, v.slice(4, -1).split(",")[0].trim())
    : v;
};

describe("accent layer: every theme x accent combination", () => {
  const ids = ["theme", ...ACCENTS.map(a => a.id)];
  it("CSS defines a dark and a light block for every registered accent, and nothing else", () => {
    expect(Object.keys(accentBlocks).sort()).toEqual(
      ACCENTS.map(a => a.id).sort()
    );
    for (const a of ACCENTS) {
      expect(accentBlocks[a.id].dark["--accent-cyan"]).toBe(a.dark);
      expect(accentBlocks[a.id].light["--accent-cyan"]).toBe(a.light);
    }
  });
  for (const t of THEMES)
    for (const a of ids) {
      it(`${t.id} x ${a}: accent text, button label and rgb triplet meet WCAG AA`, () => {
        for (const fg of ["--accent-cyan", "--accent-cyan-bright"])
          if (a !== "theme" || fg === "--accent-cyan")
            for (const bg of surfaces(t.id))
              expect([fg, bg, contrast(getA(t.id, a, fg), bg)]).toEqual([
                fg,
                bg,
                expect.toSatisfy((c: number) => c >= 4.5),
              ]);
        const fills = t.id.startsWith("x-")
          ? ["--accent-solid", "--accent-solid-hover"]
          : ["--accent-cyan", "--accent-cyan-bright"];
        for (const fill of fills)
          expect(
            contrast(getA(t.id, a, "--on-accent"), getA(t.id, a, fill))
          ).toBeGreaterThanOrEqual(4.5);
        expect(
          getA(t.id, a, "--accent-cyan-rgb")
            .split(",")
            .map(x => Number(x.trim()))
        ).toEqual(rgb(getA(t.id, a, "--accent-cyan").toLowerCase()));
      });
    }
});

describe("accent prefs: stored format stays backward compatible", () => {
  it("old prefs without `accent` parse unchanged and default to the theme accent", () => {
    const old = JSON.stringify({
      followSystem: true,
      light: "paper",
      dark: "amber",
      bgEnabled: false,
      bgIntensity: 40,
    });
    expect(parseThemePrefs(old)).toEqual({
      followSystem: true,
      light: "paper",
      dark: "amber",
      bgEnabled: false,
      bgIntensity: 40,
      accent: "theme",
    });
  });
  it("valid accent round-trips; unknown values fall back to 'theme'", () => {
    expect(parseThemePrefs(JSON.stringify({ accent: "rose" })).accent).toBe(
      "rose"
    );
    expect(parseThemePrefs(JSON.stringify({ accent: "neon" })).accent).toBe(
      "theme"
    );
    expect(parseThemePrefs(JSON.stringify({ accent: 3 })).accent).toBe("theme");
    expect(DEFAULT_THEME_PREFS.accent).toBe("theme");
    expect(isAccentId("theme")).toBe(true);
  });
  it("boot script accent list mirrors the registry", () => {
    const list = JSON.parse(
      bootSrc.match(/var A = (\[.*?\]);/)![1]
    ) as string[];
    expect(list).toEqual(ACCENTS.map(a => a.id));
  });
  it("boot script applies a stored accent and mode before first paint, ignores invalid ones", () => {
    const ok = runBoot(
      {
        "epicode-theme": "paper",
        "epicode-theme-prefs": JSON.stringify({ accent: "violet" }),
      },
      true
    );
    expect(ok.accent).toBe("violet");
    expect(ok.mode).toBe("light");
    const bad = runBoot(
      { "epicode-theme-prefs": JSON.stringify({ accent: "neon" }) },
      true
    );
    expect(bad.accent).toBeUndefined();
    expect(bad.mode).toBe("dark");
    expect(runBoot({}, true).accent).toBeUndefined();
  });
});
