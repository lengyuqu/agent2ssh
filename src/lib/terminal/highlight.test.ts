import { describe, expect, it } from "vitest";
import { compileHighlightRules, planLine, validateHighlightRule } from "./highlight";

describe("terminal highlight planning", () => {
  it("accepts regex metacharacters as literal keywords", () => {
    expect(
      validateHighlightRule({
        name: "bracket",
        keyword: "[",
        color: "#FF0000",
        enabled: true,
        is_regex: false,
        is_case_sensitive: false,
      })
    ).toBeNull();
  });

  it("rejects invalid and zero-width regular expressions", () => {
    const base = {
      name: "bad",
      color: "#FF0000",
      enabled: true,
      is_regex: true,
      is_case_sensitive: false,
    };
    expect(validateHighlightRule({ ...base, keyword: "[" })).toBe("invalid");
    expect(validateHighlightRule({ ...base, keyword: "^$" })).toBe("zero_width");
  });

  it("maps matches to terminal cells including full-width text", () => {
    const rules = compileHighlightRules([
      {
        name: "error",
        keyword: "ERROR",
        color: "#FF0000",
        enabled: true,
        is_regex: true,
        is_case_sensitive: false,
      },
    ]);
    expect(planLine("中文 ERROR", rules)).toEqual([{ x: 5, width: 5, color: "#FF0000" }]);
  });

  it("drops a swallowed match whole instead of clipping it", () => {
    // Overlap resolution keeps a maximal non-overlapping set. `word` matches at
    // 4..8, inside `password`'s 0..8, so it disappears entirely — it is not
    // narrowed to a 4-cell span at x=4. Callers must not read planLine as "the
    // union of every rule's hits".
    const rules = compileHighlightRules([
      { name: "secret", keyword: "password", color: "#AA0000", enabled: true, is_regex: false, is_case_sensitive: false },
      { name: "word", keyword: "word", color: "#00AA00", enabled: true, is_regex: false, is_case_sensitive: false },
    ]);
    expect(planLine("password", rules)).toEqual([{ x: 0, width: 8, color: "#AA0000" }]);
  });

  it("resolves overlaps by start offset, not by rule order", () => {
    // The second rule is declared later but starts at 0, so sorting by start
    // puts it first and `bcd` (1..4) is the one that gets dropped. This pins the
    // real contract — "earlier rule wins" only for equal starts.
    const rules = compileHighlightRules([
      { name: "late-start", keyword: "bcd", color: "#AA0000", enabled: true, is_regex: false, is_case_sensitive: false },
      { name: "early-start", keyword: "abc", color: "#00AA00", enabled: true, is_regex: false, is_case_sensitive: false },
    ]);
    expect(planLine("abcd", rules)).toEqual([{ x: 0, width: 3, color: "#00AA00" }]);
  });

  it("keeps disjoint matches from different rules", () => {
    const rules = compileHighlightRules([
      { name: "foo", keyword: "foo", color: "#AA0000", enabled: true, is_regex: false, is_case_sensitive: false },
      { name: "bar", keyword: "bar", color: "#00AA00", enabled: true, is_regex: false, is_case_sensitive: false },
    ]);
    expect(planLine("foo bar", rules)).toEqual([
      { x: 0, width: 3, color: "#AA0000" },
      { x: 4, width: 3, color: "#00AA00" },
    ]);
  });
});
