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
