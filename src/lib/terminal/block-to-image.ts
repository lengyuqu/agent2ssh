/**
 * Block-to-image rendering.
 *
 * Re-renders a command block's xterm cells onto an off-screen canvas and
 * exports it as a PNG blob for "copy as image" — same slice as "copy as text",
 * different renderer. Absorbed from RSSH's `block-to-image.ts`, adapted to
 * Agent2SSH's `CommandBlock.id: string` and marker-based range resolution.
 *
 * Handles:
 *   - fg/bg colors: default / ANSI-16 / 256-palette / 24-bit RGB
 *   - inverse (swap fg/bg at the data layer, zero branches in the renderer)
 *   - bold / italic / underline
 *   - CJK wide chars (width=2 occupies two columns, width=0 continuation skipped)
 *   - DPR scaling for high-DPI screens
 *
 * Deliberately omitted (low ROI for command output): dim/blink/strikethrough,
 * overline, custom underlineStyle, and per-block redaction — image copy is
 * "what you see is what you get"; token redaction belongs to the text-copy
 * path (`copyBlock` already runs `redact_for_clipboard`).
 */
import type { Terminal, ITheme, IBufferCell } from "@xterm/xterm";
import type { CommandBlock } from "./command-blocks";
import { resolveCommandBlockRange } from "./block-content";

export interface RenderOptions {
  /** 彩色竖线宽度 px。默认 4。 */
  barWidth?: number;
  /** 竖线和文字之间的间距 px。默认 10。 */
  gutter?: number;
  /** 整体外边距 px。默认 14。 */
  outerPad?: number;
}

export interface ImageCell {
  ch: string;
  /** 1 = 普通字符，2 = CJK 宽字符 */
  width: 1 | 2;
  fg: string;
  bg: string;
  bold: boolean;
  italic: boolean;
  underline: boolean;
}

export interface ImageRow {
  blockId: string;
  blockColor: string;
  /** xterm soft-wrap continuation; true means this row continues the previous. */
  isWrapped: boolean;
  cells: ImageCell[];
}

/** Render selected blocks to a PNG Blob. Empty set / non-DOM env / render
 *  failure → null. */
export async function renderBlocksToBlob(
  term: Terminal,
  blocks: ReadonlyArray<CommandBlock>,
  opts: RenderOptions = {},
): Promise<Blob | null> {
  if (blocks.length === 0) return null;
  if (typeof document === "undefined") return null;
  if (document.fonts?.ready) {
    await document.fonts.ready;
  }
  const rows = extractImageRows(term, blocks);
  if (rows.length === 0) return null;
  return renderRowsToBlob(rows, term, opts);
}

/* ───────────────────────── 数据抽取 ───────────────────────── */

/** Selected blocks → visual row sequence. Soft-wrapped rows are NOT merged
 *  (an image is a "screenshot" and keeps the visual layout). */
export function extractImageRows(
  term: Terminal,
  blocks: ReadonlyArray<CommandBlock>,
): ImageRow[] {
  const sorted = [...blocks]
    .filter((block) => !block.start.isDisposed)
    .sort((a, b) => a.start.line - b.start.line);
  if (sorted.length === 0) return [];
  const theme: ITheme = term.options.theme ?? {};
  const out: ImageRow[] = [];
  for (const block of sorted) {
    const range = resolveCommandBlockRange(term, block);
    if (!range) continue;
    const buffer = term.buffer.normal;
    for (let line = range.startLine; line <= range.endLine; line++) {
      const value = buffer.getLine(line);
      if (!value) continue;
      const cells: ImageCell[] = [];
      for (let x = 0; x < value.length; x++) {
        const cell = value.getCell(x);
        if (!cell || cell.getWidth() === 0) continue;
        cells.push(cellToImageCell(cell, theme));
      }
      // Trim trailing "space + default background" so the image stays narrow.
      while (cells.length > 0) {
        const last = cells[cells.length - 1];
        if (last.ch === " " && last.bg === defaultBg(theme)) cells.pop();
        else break;
      }
      out.push({
        blockId: block.id,
        blockColor: block.color,
        isWrapped: value.isWrapped,
        cells,
      });
    }
  }
  return out;
}

function cellToImageCell(cell: IBufferCell, theme: ITheme): ImageCell {
  let fg = resolveFg(cell, theme);
  let bg = resolveBg(cell, theme);
  if (cell.isInverse()) {
    const t = fg;
    fg = bg;
    bg = t;
  }
  return {
    ch: cell.getChars() || " ",
    width: cell.getWidth() === 2 ? 2 : 1,
    fg,
    bg,
    bold: !!cell.isBold(),
    italic: !!cell.isItalic(),
    underline: !!cell.isUnderline(),
  };
}

/* ───────────────────────── 颜色解析 ───────────────────────── */

export function resolveFg(cell: IBufferCell, theme: ITheme): string {
  if (cell.isFgDefault()) return defaultFg(theme);
  if (cell.isFgRGB()) return rgbFromInt(cell.getFgColor());
  if (cell.isFgPalette()) return paletteToColor(cell.getFgColor(), theme, defaultFg(theme));
  return defaultFg(theme);
}

export function resolveBg(cell: IBufferCell, theme: ITheme): string {
  if (cell.isBgDefault()) return defaultBg(theme);
  if (cell.isBgRGB()) return rgbFromInt(cell.getBgColor());
  if (cell.isBgPalette()) return paletteToColor(cell.getBgColor(), theme, defaultBg(theme));
  return defaultBg(theme);
}

function defaultFg(theme: ITheme): string {
  return theme.foreground ?? "#ffffff";
}

function defaultBg(theme: ITheme): string {
  return theme.background ?? "#000000";
}

function rgbFromInt(n: number): string {
  const r = (n >> 16) & 0xff;
  const g = (n >> 8) & 0xff;
  const b = n & 0xff;
  return `rgb(${r},${g},${b})`;
}

export function paletteToColor(idx: number, theme: ITheme, fallback: string): string {
  if (idx < 0) return fallback;
  if (idx < 16) return ansi16(idx, theme);
  if (idx < 232) return ansi256Cube(idx);
  if (idx < 256) return ansi256Gray(idx);
  return fallback;
}

function ansi16(idx: number, theme: ITheme): string {
  const slots: (string | undefined)[] = [
    theme.black, theme.red, theme.green, theme.yellow,
    theme.blue, theme.magenta, theme.cyan, theme.white,
    theme.brightBlack, theme.brightRed, theme.brightGreen, theme.brightYellow,
    theme.brightBlue, theme.brightMagenta, theme.brightCyan, theme.brightWhite,
  ];
  return slots[idx] ?? "#888888";
}

function ansi256Cube(idx: number): string {
  const i = idx - 16;
  const r = (i / 36) | 0;
  const g = ((i / 6) | 0) % 6;
  const b = i % 6;
  const conv = (x: number) => (x === 0 ? 0 : 55 + x * 40);
  return `rgb(${conv(r)},${conv(g)},${conv(b)})`;
}

function ansi256Gray(idx: number): string {
  const v = 8 + (idx - 232) * 10;
  return `rgb(${v},${v},${v})`;
}

/* ───────────────────────── Canvas 渲染 ───────────────────────── */

interface BlockGroup {
  blockId: string;
  color: string;
  startRow: number;
  rowCount: number;
}

function groupRows(rows: ImageRow[]): BlockGroup[] {
  const out: BlockGroup[] = [];
  let cur: BlockGroup | null = null;
  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    if (!cur || cur.blockId !== row.blockId) {
      cur = { blockId: row.blockId, color: row.blockColor, startRow: i, rowCount: 1 };
      out.push(cur);
    } else {
      cur.rowCount++;
    }
  }
  return out;
}

async function renderRowsToBlob(
  rows: ImageRow[],
  term: Terminal,
  opts: RenderOptions,
): Promise<Blob | null> {
  const fontSize = term.options.fontSize ?? 13;
  const fontFamily = term.options.fontFamily ?? "monospace";
  const lineHeightMul = term.options.lineHeight ?? 1.0;
  const lineHeight = Math.round(fontSize * Math.max(lineHeightMul, 1.0) * 1.3);
  const theme: ITheme = term.options.theme ?? {};
  const bgColor = defaultBg(theme);

  const outerPad = opts.outerPad ?? 14;
  const barWidth = opts.barWidth ?? 4;
  const gutter = opts.gutter ?? 10;

  const measureCanvas = document.createElement("canvas");
  const mctx = measureCanvas.getContext("2d");
  if (!mctx) return null;
  mctx.font = `${fontSize}px ${fontFamily}`;
  const cellWidth = Math.max(1, Math.ceil(mctx.measureText("M").width));

  let maxCells = 0;
  for (const row of rows) {
    let w = 0;
    for (const cell of row.cells) w += cell.width;
    if (w > maxCells) maxCells = w;
  }
  if (maxCells === 0) maxCells = 1;

  const groups = groupRows(rows);
  const canvasW = outerPad * 2 + barWidth + gutter + maxCells * cellWidth;
  const canvasH = outerPad * 2 + rows.length * lineHeight;

  const dpr = (typeof window !== "undefined" && window.devicePixelRatio) || 1;
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(canvasW * dpr);
  canvas.height = Math.ceil(canvasH * dpr);
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.scale(dpr, dpr);

  ctx.fillStyle = bgColor;
  ctx.fillRect(0, 0, canvasW, canvasH);
  ctx.textBaseline = "top";

  const textStartX = outerPad + barWidth + gutter;
  for (const group of groups) {
    const barX = outerPad;
    const barY = outerPad + group.startRow * lineHeight;
    const barH = group.rowCount * lineHeight;
    ctx.fillStyle = group.color;
    roundRect(ctx, barX, barY, barWidth, barH, barWidth / 2);
    ctx.fill();

    for (let i = 0; i < group.rowCount; i++) {
      const row = rows[group.startRow + i];
      const rowY = outerPad + (group.startRow + i) * lineHeight;
      drawRow(ctx, row, rowY, lineHeight, cellWidth, fontSize, fontFamily, textStartX, bgColor);
    }
  }

  return new Promise((resolve) => {
    canvas.toBlob((blob) => resolve(blob), "image/png");
  });
}

function drawRow(
  ctx: CanvasRenderingContext2D,
  row: ImageRow,
  y: number,
  lineHeight: number,
  cellWidth: number,
  fontSize: number,
  fontFamily: string,
  startX: number,
  defaultBgColor: string,
) {
  // Pass 1: background — merge contiguous same-color runs, skip default bg.
  let x = startX;
  let runStart = x;
  let runColor: string | null = null;
  const flush = (endX: number) => {
    if (runColor !== null && runColor !== defaultBgColor) {
      ctx.fillStyle = runColor;
      ctx.fillRect(runStart, y, endX - runStart, lineHeight);
    }
  };
  for (const cell of row.cells) {
    const cw = cellWidth * cell.width;
    if (runColor === null) {
      runColor = cell.bg;
      runStart = x;
    } else if (cell.bg !== runColor) {
      flush(x);
      runColor = cell.bg;
      runStart = x;
    }
    x += cw;
  }
  flush(x);

  // Pass 2: characters.
  x = startX;
  const textY = y + Math.max(0, (lineHeight - fontSize) / 2);
  for (const cell of row.cells) {
    const cw = cellWidth * cell.width;
    if (cell.ch !== " " && cell.ch.trim() !== "") {
      const weight = cell.bold ? "bold" : "normal";
      const style = cell.italic ? "italic" : "normal";
      ctx.font = `${style} ${weight} ${fontSize}px ${fontFamily}`;
      ctx.fillStyle = cell.fg;
      ctx.fillText(cell.ch, x, textY);
    }
    x += cw;
  }

  // Pass 3: underline.
  x = startX;
  for (const cell of row.cells) {
    const cw = cellWidth * cell.width;
    if (cell.underline) {
      ctx.fillStyle = cell.fg;
      ctx.fillRect(x, y + lineHeight - 2, cw, 1);
    }
    x += cw;
  }
}

function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  const rad = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rad, y);
  ctx.lineTo(x + w - rad, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + rad);
  ctx.lineTo(x + w, y + h - rad);
  ctx.quadraticCurveTo(x + w, y + h, x + w - rad, y + h);
  ctx.lineTo(x + rad, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - rad);
  ctx.lineTo(x, y + rad);
  ctx.quadraticCurveTo(x, y, x + rad, y);
  ctx.closePath();
}
