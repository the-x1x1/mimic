import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * A theme that is missing a token does not fail: it silently inherits the
 * previous theme's value, so a single forgotten line gives you near-black text
 * on near-black. That is the failure these guard against, along with the one
 * idea the look is built on — writing is set in the serif — which is easy to
 * undo by accident the next time someone tidies a stylesheet.
 */
const root = join(__dirname, "..", "..", "..", "..", "packages", "ui", "src");
const tokens = readFileSync(join(root, "tokens.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const ui = readFileSync(join(root, "ui.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");

/** The custom properties declared by the rule for `selector`. */
function declared(selector: string): Set<string> {
  const names = new Set<string>();
  for (const [, selectors, body] of tokens.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (!(selectors ?? "").split(",").some((s) => s.trim() === selector)) continue;
    for (const [, name] of (body ?? "").matchAll(/(--[a-z0-9-]+)\s*:/g)) names.add(name!);
  }
  return names;
}

describe("the three themes are one token set", () => {
  const base = declared(":root");

  it("has a base theme that defines the whole vocabulary", () => {
    expect(base.size).toBeGreaterThan(30);
    for (const essential of [
      "--bg",
      "--surface-1",
      "--text-primary",
      "--accent",
      "--on-accent",
      "--font-sans",
      "--font-serif",
      "--text-letter",
      "--card-border",
      "--card-padding",
    ]) {
      expect(base.has(essential), `:root is missing ${essential}`).toBe(true);
    }
  });

  it("gives every theme its own colours, not the previous theme's", () => {
    // Colour and surface tokens cascade visibly wrong when forgotten; the type
    // and spacing scale is deliberately shared, so it is not required here.
    const mustOverride = [
      "--bg",
      "--surface-1",
      "--surface-2",
      "--surface-3",
      "--border",
      "--border-strong",
      "--border-control",
      "--text-primary",
      "--text-secondary",
      "--text-muted",
      "--accent",
      "--accent-strong",
      "--accent-soft",
      "--on-accent",
      "--success",
      "--warning",
      "--danger",
      "--focus",
    ];
    for (const theme of ['[data-theme="paper"]', '[data-theme="night"]']) {
      const own = declared(theme);
      expect(own.size, `${theme} declares nothing`).toBeGreaterThan(0);
      for (const name of mustOverride) {
        expect(own.has(name), `${theme} inherits ${name} from the theme above it`).toBe(true);
      }
    }
  });

  it("lets paper draw rules where the others draw boxes, through a token", () => {
    // Not by branching on the theme name in a component.
    expect(declared('[data-theme="paper"]').has("--card-border")).toBe(true);
    expect(declared('[data-theme="paper"]').has("--card-padding")).toBe(true);
    expect(ui).toMatch(/\.ui-card\s*\{[^}]*border:\s*var\(--card-border\)/);
  });
});

describe("the serif carries the writing", () => {
  it("sets a letter in the serif and at the letter size", () => {
    const letter = ui.match(/(^|\})\s*\.letter\s*\{([^}]*)\}/m);
    expect(letter, "there is no .letter rule").not.toBeNull();
    expect(letter![2]).toMatch(/font-family:\s*var\(--font-serif\)/);
    expect(letter![2]).toMatch(/font-size:\s*var\(--text-letter\)/);
  });

  it("keeps the app itself in the sans", () => {
    expect(tokens).toMatch(/--font-sans:\s*"IBM Plex Sans"/);
    expect(tokens).toMatch(/--font-serif:\s*"IBM Plex Serif"/);
  });
});

/** Every colour token a theme ends up with: the base theme's, then its own. */
function colours(selector: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const sel of [":root", selector]) {
    for (const [, selectors, body] of tokens.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
      if (!(selectors ?? "").split(",").some((s) => s.trim() === sel)) continue;
      for (const [, name, hex] of (body ?? "").matchAll(
        /(--[a-z0-9-]+)\s*:\s*(#[0-9a-fA-F]{6})\b/g,
      )) {
        out.set(name!, hex!);
      }
    }
  }
  return out;
}

/** WCAG 2's contrast ratio between two #rrggbb colours. */
function contrast(a: string, b: string): number {
  const lum = (hex: string) => {
    const [r, g, bl] = [1, 3, 5].map((i) => {
      const c = parseInt(hex.slice(i, i + 2), 16) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r! + 0.7152 * g! + 0.0722 * bl!;
  };
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
}

describe("what has to be read or found can be, in every theme", () => {
  const themes = [":root", '[data-theme="paper"]', '[data-theme="night"]'];
  const grounds = ["--bg", "--surface-1", "--surface-2"];

  it("keeps text at 4.5:1 or better on the grounds it is set on", () => {
    for (const theme of themes) {
      const c = colours(theme);
      for (const fg of [
        "--text-primary",
        "--text-secondary",
        "--text-muted",
        "--accent",
        "--danger",
      ]) {
        for (const bg of grounds) {
          const ratio = contrast(c.get(fg)!, c.get(bg)!);
          expect(ratio, `${theme} ${fg} on ${bg} is ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(
            4.5,
          );
        }
      }
      expect(contrast(c.get("--on-accent")!, c.get("--accent")!)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("draws the edge of a text box at 3:1 or better, so the box can be found", () => {
    for (const theme of themes) {
      const c = colours(theme);
      for (const bg of grounds) {
        const ratio = contrast(c.get("--border-control")!, c.get(bg)!);
        expect(
          ratio,
          `${theme} --border-control on ${bg} is ${ratio.toFixed(2)}:1`,
        ).toBeGreaterThanOrEqual(3);
      }
    }
  });
});
