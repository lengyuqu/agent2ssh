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
});
