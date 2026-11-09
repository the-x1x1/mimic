import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * These are not rendering tests — jsdom does not lay anything out, and a real
 * check would need a browser. They guard the three declarations that, when
 * they went missing, produced bugs nobody would think to look for in a
 * stylesheet: the whole window scrolling so that the navigation left the
 * screen, and drop-downs rendering as white system combo boxes in a dark app.
 * A declaration that only matters when it is absent is worth pinning down.
 */
const css = readFileSync(join(__dirname, "global.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");

/**
 * Every declaration that applies to `selector`, from all the rules that name
 * it — `select` is styled by two, one shared with the other text controls and
 * one of its own.
 */
function declarations(selector: string): string {
  let out = "";
  for (const [, selectors, body] of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if ((selectors ?? "").split(",").some((s) => s.trim() === selector)) out += body ?? "";
  }
  expect(out, `no rule names ${selector}`).not.toEqual("");
  return out;
}

describe("the app is the window, and only the content scrolls", () => {
  it("keeps the scrolling pane from growing past its row", () => {
    // Without this the content pane cannot scroll: it grows instead, and the
    // top bar is the first thing to leave the screen.
    expect(declarations(".app")).toMatch(/grid-template-rows:\s*auto minmax\(0, 1fr\)/);
    expect(declarations(".app")).toMatch(/overflow:\s*hidden/);
    expect(declarations(".content")).toMatch(/min-height:\s*0/);
    expect(declarations(".content")).toMatch(/overflow:\s*auto/);
  });

  it("holds page content to one measure", () => {
    expect(declarations(".content__inner")).toMatch(/max-width:/);
  });
});

describe("a screen that can outgrow the window scrolls", () => {
  it("lets setup scroll, since body does not", () => {
    // body has overflow:hidden, so a full-height screen that grows past the
    // viewport has no scrollbar and everything below the fold is unreachable.
    // That is how setup lost its buttons the moment a step was added to it.
    const onboarding = declarations(".onboarding");
    expect(onboarding).toMatch(/overflow-y:\s*auto/);
    expect(onboarding).toMatch(/height:\s*100%/);
    // Centring a tall page pushes its first step off the top of the screen.
    expect(onboarding).toMatch(/align-content:\s*start/);
  });

  it("defines each full-height screen exactly once", () => {
    // The bug above was really two .onboarding rules, the later one quietly
    // winning. A rule declared twice is a rule nobody can reason about.
    for (const selector of [".onboarding", ".app", ".content", ".drawer"]) {
      const count = [...css.matchAll(/([^{}]+)\{[^{}]*\}/g)].filter(([, sel]) =>
        (sel ?? "").split(",").some((s) => s.trim() === selector),
      ).length;
      expect(count, `${selector} is declared ${count} times`).toBe(1);
    }
  });
});

describe("no control falls back to the system's own styling", () => {
  it("styles select, textarea and text inputs together with .input", () => {
    // An <input> with no type is a text input; fourteen of them were left to
    // the system because the rule named only [type="text"].
    for (const el of ["select", "textarea", 'input[type="text"]', "input:not([type])"]) {
      expect(declarations(el), `${el} is left to the system`).toMatch(/border:/);
    }
  });

  it("draws a text control's edge in the colour made to be seen", () => {
    expect(declarations("textarea")).toMatch(/border:\s*1px solid var\(--border-control\)/);
  });

  it("draws the drop-down arrow back after removing the system one", () => {
    const select = declarations("select");
    expect(select).toMatch(/appearance:\s*none/);
    expect(select).toMatch(/background-image:/);
  });
});

describe("nothing in the drawer or a dialog scrolls sideways", () => {
  it("makes the drawer wide enough for a page with a table in it", () => {
    const width = /width:\s*min\((\d+)px/.exec(declarations(".drawer"));
    expect(width, "the drawer has no min() width").not.toBeNull();
    expect(Number(width![1])).toBeGreaterThanOrEqual(720);
  });

  it("lets a page's actions go under its title rather than squeeze it", () => {
    expect(declarations(".page-header")).toMatch(/flex-wrap:\s*wrap/);
    expect(declarations(".page-header__text")).toMatch(/min-width:\s*0/);
  });

  it("breaks a long path or address instead of widening a dialog or a table", () => {
    expect(declarations(".dialog")).toMatch(/overflow-wrap:\s*anywhere/);
    expect(declarations(".dialog")).not.toMatch(/overflow:\s*auto/);
    // In a table, break-word: `anywhere` lets a table squeeze its columns
    // until every word breaks.
    expect(declarations(".table td")).toMatch(/overflow-wrap:\s*break-word/);
  });
});
