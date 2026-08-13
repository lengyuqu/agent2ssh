import { describe, it, expect } from "vitest";
import {
  paletteToColor,
  resolveBg,
  resolveFg,
  type ImageCell,
} from "./block-to-image";

/** Minimal ITheme for color resolution tests. */
const theme = {
  foreground: "#ffffff",
  background: "#000000",
  black: "#000000",
  red: "#cc0000",
  green: "#00cc00",
  yellow: "#cccc00",
  blue: "#0000cc",
  magenta: "#cc00cc",
  cyan: "#00cccc",
  white: "#cccccc",
  brightBlack: "#555555",
  brightRed: "#ff5555",
  brightGreen: "#55ff55",
  brightYellow: "#ffff55",
  brightBlue: "#5555ff",
  brightMagenta: "#ff55ff",
  brightCyan: "#55ffff",
  brightWhite: "#ffffff",
};

function cellWith(
  methods: Partial<{
    isFgDefault: () => boolean;
    isBgDefault: () => boolean;
    isFgRGB: () => boolean;
    isBgRGB: () => boolean;
    isFgPalette: () => boolean;
    isBgPalette: () => boolean;
    getFgColor: () => number;
    getBgColor: () => number;
  }>,
) {
  const defaults = {
    isFgDefault: () => false,
    isBgDefault: () => false,
    isFgRGB: () => false,
    isBgRGB: () => false,
    isFgPalette: () => false,
    isBgPalette: () => false,
    getFgColor: () => 0,
    getBgColor: () => 0,
  };
  return { ...defaults, ...methods } as never;
}

describe("block-to-image — color resolution", () => {
  it("resolves default fg/bg from theme", () => {
    const cell = cellWith({ isFgDefault: () => true, isBgDefault: () => true });
    expect(resolveFg(cell, theme)).toBe("#ffffff");
    expect(resolveBg(cell, theme)).toBe("#000000");
  });

  it("resolves 24-bit RGB", () => {
    const cell = cellWith({
      isFgRGB: () => true,
      getFgColor: () => 0xff8040,
      isBgRGB: () => true,
      getBgColor: () => 0x102030,
    });
    expect(resolveFg(cell, theme)).toBe("rgb(255,128,64)");
    expect(resolveBg(cell, theme)).toBe("rgb(16,32,48)");
  });

  it("resolves ANSI-16 palette via theme slots", () => {
    const cell = cellWith({ isFgPalette: () => true, getFgColor: () => 1 });
    expect(resolveFg(cell, theme)).toBe("#cc0000");
  });

  it("resolves 256-palette cube and gray ramp", () => {
    // idx 16 → cube(0,0,0) → rgb(0,0,0); idx 231 → cube(5,5,5) → rgb(255,255,255)
    expect(paletteToColor(16, theme, "#fff")).toBe("rgb(0,0,0)");
    expect(paletteToColor(231, theme, "#fff")).toBe("rgb(255,255,255)");
    // idx 232 → gray(8); idx 255 → gray(238)
    expect(paletteToColor(232, theme, "#fff")).toBe("rgb(8,8,8)");
    expect(paletteToColor(255, theme, "#fff")).toBe("rgb(238,238,238)");
  });

  it("falls back for out-of-range palette index", () => {
    expect(paletteToColor(-1, theme, "#ffffff")).toBe("#ffffff");
    expect(paletteToColor(256, theme, "#ffffff")).toBe("#ffffff");
  });
});

describe("block-to-image — ImageCell width", () => {
  it("exposes width field contract", () => {
    const cell: ImageCell = {
      ch: "a",
      width: 1,
      fg: "#fff",
      bg: "#000",
      bold: false,
      italic: false,
      underline: false,
    };
    expect(cell.width).toBe(1);
  });
});
