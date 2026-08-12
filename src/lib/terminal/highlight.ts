/**
 * B24: Terminal highlight — pure matching logic (no xterm dependency).
 *
 * Ported from rssh's `highlight.ts`, adapted for agent2ssh. This module
 * provides rule validation, compilation, match finding, and line planning.
 * The actual xterm decoration is applied in `highlight-decorations.ts`.
 */

/** A compiled highlight rule ready for matching. */
export interface CompiledHighlightRule {
  keyword: string;
  name: string;
  color: string;
  enabled: boolean;
  is_regex: boolean;
  is_case_sensitive: boolean;
  source: string;
  regex: RegExp | null;
}

/** Raw highlight rule from the backend. */
export interface HighlightRule {
  keyword: string;
  name: string;
  color: string;
  enabled: boolean;
  is_regex: boolean;
  is_case_sensitive: boolean;
}

export type HighlightValidationError =
  | "invalid"
  | "zero_width"
  | "name_required"
  | "name_too_long";

/** A single match within a line of terminal text. */
export interface HighlightMatch {
  start: number;
  end: number;
  color: string;
  rule: CompiledHighlightRule;
}

/** A decoration plan for a line (cell-based positions). */
export interface LineDecoration {
  x: number;
  width: number;
  color: string;
}

/**
 * Detect whether a regex pattern consists solely of zero-width assertions
 * (anchors, lookarounds) — such patterns would match at every position
 * without consuming characters, causing infinite loops.
 */
function isPureZeroWidth(source: string): boolean {
  // Remove all anchors, lookaheads, lookbehinds, and word boundaries
  const stripped = source
    .replace(/\\b/g, "")
    .replace(/\(\?<[=!].*?\)/g, "")
    .replace(/\(\?[=!].*?\)/g, "")
    .replace(/[\^$]/g, "");
  return stripped.length === 0;
}

/**
 * Validate a highlight rule's regex and name.
 * Returns `null` if valid, or an error code.
 */
export function validateHighlightRule(
  rule: HighlightRule,
): HighlightValidationError | null {
  if (!rule.name || rule.name.trim().length === 0) {
    return "name_required";
  }
  if (rule.name.length > 100) {
    return "name_too_long";
  }
  try {
    const flags = rule.is_case_sensitive ? "g" : "gi";
    new RegExp(rule.keyword, flags);
  } catch {
    return "invalid";
  }
  if (isPureZeroWidth(rule.keyword)) {
    return "zero_width";
  }
  return null;
}

/**
 * Compile a list of raw rules into compiled rules. Invalid regexes get
 * `regex: null` and are silently skipped during matching.
 */
export function compileHighlightRules(
  rules: HighlightRule[],
): CompiledHighlightRule[] {
  return rules
    .filter((r) => r.enabled)
    .map((rule) => {
      const compiled: CompiledHighlightRule = {
        ...rule,
        source: rule.keyword,
        regex: null,
      };
      try {
        const flags = rule.is_case_sensitive ? "g" : "gi";
        compiled.regex = new RegExp(rule.keyword, flags);
      } catch {
        // Invalid regex — skip this rule
        compiled.regex = null;
      }
      return compiled;
    })
    .filter((r) => r.regex !== null);
}

/**
 * Find all matches in a plain-text string. Overlaps are resolved by
 * earlier-rule-wins ordering. Zero-width matches are skipped.
 */
export function findMatches(
  text: string,
  compiled: CompiledHighlightRule[],
): HighlightMatch[] {
  const matches: HighlightMatch[] = [];

  for (const rule of compiled) {
    if (!rule.regex) continue;
    const regex = new RegExp(rule.source, rule.is_case_sensitive ? "g" : "gi");
    let m: RegExpExecArray | null;
    while ((m = regex.exec(text)) !== null) {
      const start = m.index;
      const end = start + m[0].length;
      if (end === start) {
        // Zero-width match — advance to avoid infinite loop
        regex.lastIndex++;
        continue;
      }
      matches.push({ start, end, color: rule.color, rule });
    }
  }

  // Sort by start position, then by rule order (earlier wins for overlaps)
  matches.sort((a, b) => a.start - b.start || 0);

  // Resolve overlaps: earlier match wins
  const result: HighlightMatch[] = [];
  let lastEnd = 0;
  for (const m of matches) {
    if (m.start >= lastEnd) {
      result.push(m);
      lastEnd = m.end;
    }
  }

  return result;
}

/** Maps a UTF-16 string offset to a terminal cell column. */
function offsetToCell(text: string, offset: number): number {
  let cell = 0;
  for (let i = 0; i < offset && i < text.length; i++) {
    const code = text.charCodeAt(i);
    // Surrogate pair — count as 1 cell (emoji)
    if (code >= 0xd800 && code <= 0xdbff && i + 1 < text.length) {
      i++;
      cell++;
      continue;
    }
    // CJK and fullwidth ranges — 2 cells
    if (
      (code >= 0x1100 && code <= 0x115f) || // Hangul Jamo
      (code >= 0x2e80 && code <= 0x9fff) || // CJK
      (code >= 0xac00 && code <= 0xd7a3) || // Hangul Syllables
      (code >= 0xf900 && code <= 0xfaff) || // CJK Compat
      (code >= 0xfe30 && code <= 0xfe4f) || // CJK Compat Forms
      (code >= 0xff00 && code <= 0xff60) || // Fullwidth Forms
      (code >= 0xffe0 && code <= 0xffe6)
    ) {
      cell += 2;
    } else {
      cell++;
    }
  }
  return cell;
}

/**
 * Plan decorations for a line of terminal text. Converts character-offset
 * matches to cell-based positions for xterm decoration.
 */
export function planLine(
  text: string,
  compiled: CompiledHighlightRule[],
): LineDecoration[] {
  const matches = findMatches(text, compiled);
  return matches.map((m) => ({
    x: offsetToCell(text, m.start),
    width: offsetToCell(text, m.end) - offsetToCell(text, m.start),
    color: m.color,
  }));
}
