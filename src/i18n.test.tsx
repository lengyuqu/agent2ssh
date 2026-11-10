/// <reference types="vite/client" />
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider, useI18n, zh } from "./i18n";

const mockApi = vi.hoisted(() => ({
  setTrayLabels: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("./api", () => ({ api: mockApi, reportError: vi.fn() }));

/**
 * Guards for the dictionary itself.
 *
 * `zh` is keyed by the English source text, so a key that does not match its
 * `t()` call site byte for byte fails silently: `t()` falls back to the key and
 * the Chinese UI shows English. `nav.test.ts` only covers the twenty sidebar
 * labels, which left the other ~700 call sites unguarded — the gap raised as
 * items 4 and 10 on issue #10.
 *
 * Scope, deliberately narrow: this scans *string literals* handed to `t()`.
 * Keys that reach `t()` through a variable are covered only when the literal is
 * visible at the call site (the `t(cond ? "A" : "B")` shape is; object tables
 * read as `t(entry.label)` are not). A key reachable only through a table can
 * still go missing unnoticed; what this closes is the class where the literal
 * and the dictionary disagree.
 *
 * Sources come from a build-time raw glob rather than `node:fs`: this project
 * compiles with `lib: ["DOM", …]` and ships no `@types/node`, so importing
 * `node:fs` here would be the only `tsc` error in the tree.
 */
const SOURCES = import.meta.glob("./**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

// `\"` inside a double-quoted literal and a bare `"` inside a single-quoted one
// are the same character at runtime; the dictionary must be compared after that
// unescaping, or every quoted string in the UI reads as a missing key.
const ESCAPES: Record<string, string> = { n: "\n", r: "\r", t: "\t", "0": "\0" };
function unescape(raw: string): string {
  return raw.replace(/\\(.)/g, (_match, char: string) => ESCAPES[char] ?? char);
}

const CALL = /\bt\(\s*(?:"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)')/g;

type CallSite = { key: string; where: string };

/** Line number for a character offset, so a hit can be reported where it starts. */
function lineOf(text: string, offset: number): number {
  return text.slice(0, offset).split("\n").length;
}

function literalCallSites(): CallSite[] {
  const sites: CallSite[] = [];
  for (const [file, text] of Object.entries(SOURCES)) {
    if (/\.test\.(ts|tsx)$/.test(file)) continue;
    // Scan the whole file, not line by line: `t(` and its literal are
    // routinely split across lines by prettier (26 sites did when this was
    // written, all in long help strings), and a per-line scan drops them
    // without a word -- the key then goes unchecked for good.
    CALL.lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = CALL.exec(text)) !== null) {
      sites.push({
        key: unescape(match[1] ?? match[2] ?? ""),
        where: `${file.replace(/^\.\//, "")}:${lineOf(text, match.index)}`,
      });
    }
  }
  return sites;
}

const PLACEHOLDER = /\{([A-Za-z0-9_]+)\}/g;
function placeholders(text: string): Set<string> {
  return new Set(Array.from(text.matchAll(PLACEHOLDER), (match) => match[1]));
}

const has = (key: string) => Object.prototype.hasOwnProperty.call(zh, key);

describe("i18n dictionary", () => {
  it("finds the call sites it is supposed to check", () => {
    // A guard that silently scans nothing is worth less than no guard: this
    // pins the scanner. The exact count moves as the UI grows; the floor is
    // what matters. 900 rather than 600 because a per-line scan (the shape
    // this replaced) sees only 922 of the 948 sites, so a floor of 600 has
    // enough headroom to hide that whole class of regression.
    expect(literalCallSites().length).toBeGreaterThan(900);
  });

  it("checks a t() call whose literal starts on the next line", () => {
    // The scan is whole-file for this reason: prettier splits long help
    // strings after `t(`, and a per-line scan silently skips them.
    const sites = literalCallSites();
    const multiline = sites.filter((site) =>
      site.key.startsWith("The saved SSH fingerprint for this host")
    );
    expect(multiline).toHaveLength(1);
  });

  it("has a zh entry for every literal t() key", () => {
    const missing = literalCallSites()
      .filter((site) => !has(site.key))
      .map((site) => `${site.where} -> ${JSON.stringify(site.key)}`);
    expect(missing).toEqual([]);
  });

  it("keeps the same {placeholders} on both sides of every entry", () => {
    // A `{count}` translated as `{n}` leaves the literal `{n}` on screen, and a
    // placeholder dropped from the Chinese side silently loses the number.
    const mismatched: string[] = [];
    for (const [key, value] of Object.entries(zh)) {
      const inKey = placeholders(key);
      const inValue = placeholders(value);
      const onlyKey = Array.from(inKey).filter((name) => !inValue.has(name));
      const onlyValue = Array.from(inValue).filter((name) => !inKey.has(name));
      if (onlyKey.length > 0 || onlyValue.length > 0) {
        mismatched.push(
          `${JSON.stringify(key)}: only-in-key=[${onlyKey}] only-in-value=[${onlyValue}]`
        );
      }
    }
    expect(mismatched).toEqual([]);
  });
});

function Probe() {
  const { language, setLanguage, t } = useI18n();
  return (
    <>
      <span data-testid="language">{language}</span>
      <span data-testid="cancel">{t("Cancel")}</span>
      <span data-testid="files">{t("{count} files", { count: 3 })}</span>
      <span data-testid="partial">{t("Only {one} of {two}", { one: "a" })}</span>
      <button type="button" onClick={() => setLanguage("zh")}>
        switch
      </button>
    </>
  );
}

function renderProbe() {
  return render(
    <I18nProvider>
      <Probe />
    </I18nProvider>
  );
}

describe("language selection", () => {
  const setBrowserLanguage = (value: string) => {
    Object.defineProperty(window.navigator, "language", { value, configurable: true });
  };

  beforeEach(() => {
    localStorage.clear();
    setBrowserLanguage("en-US");
  });

  it("prefers a stored choice over the browser language", () => {
    localStorage.setItem("agent2ssh.language", "zh");
    setBrowserLanguage("en-US");
    renderProbe();
    expect(screen.getByTestId("language").textContent).toContain("zh");
    expect(screen.getByTestId("cancel").textContent).toContain("取消");
  });

  it("falls back to the browser language when nothing is stored", () => {
    setBrowserLanguage("zh-CN");
    renderProbe();
    expect(screen.getByTestId("language").textContent).toContain("zh");
  });

  it("ignores a stored value that is neither language", () => {
    localStorage.setItem("agent2ssh.language", "fr");
    setBrowserLanguage("zh-CN");
    renderProbe();
    expect(screen.getByTestId("language").textContent).toContain("zh");
  });

  it("persists a switch so the next mount reads it back", () => {
    renderProbe();
    expect(screen.getByTestId("language").textContent).toContain("en");
    fireEvent.click(screen.getByText("switch"));
    expect(screen.getByTestId("language").textContent).toContain("zh");
    expect(localStorage.getItem("agent2ssh.language")).toBe("zh");
  });

  it("translates and interpolates through t()", () => {
    localStorage.setItem("agent2ssh.language", "zh");
    renderProbe();
    expect(screen.getByTestId("files").textContent).toContain("3 个文件");
    // An unknown key is passed through verbatim, and a variable that was not
    // supplied keeps its braces rather than rendering "undefined".
    expect(screen.getByTestId("partial").textContent).toContain("Only a of {two}");
  });
});
