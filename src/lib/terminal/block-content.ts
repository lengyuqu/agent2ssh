import type { IBufferLine, Terminal } from "@xterm/xterm";
import type { CommandBlock } from "./command-blocks";

export const COMMAND_BLOCK_COPY_MAX_CHARS = 1_000_000;

export interface CommandBlockMetadata {
  id: string;
  host: string;
  command: string | null;
  color: string;
  startLine: number;
  endLine: number;
  startedAt: string;
  endedAt: string | null;
  active: boolean;
}

export class CommandBlockTextLimitError extends Error {
  constructor(readonly limit: number) {
    super(`command_block_text_exceeds_${limit}_characters`);
    this.name = "CommandBlockTextLimitError";
  }
}

function cleanCellText(text: string): string {
  // xterm cells contain rendered glyphs rather than ANSI, but strip C0/C1 and
  // ESC defensively so copied output can never recreate a control sequence.
  let clean = "";
  for (const character of text) {
    const code = character.charCodeAt(0);
    if (code <= 8 || (code >= 11 && code <= 31) || (code >= 127 && code <= 159)) continue;
    clean += character;
  }
  return clean;
}

function lineText(line: IBufferLine): string {
  let text = "";
  for (let column = 0; column < line.length; column++) {
    const cell = line.getCell(column);
    if (!cell || cell.getWidth() === 0) continue;
    const rendered = cell.getChars();
    text += rendered ? cleanCellText(rendered) : " ";
  }
  return text;
}

/** Convert physical xterm rows to text while joining soft-wrapped rows. */
export function linesToLogicalText(lines: ReadonlyArray<IBufferLine>, maxChars = COMMAND_BLOCK_COPY_MAX_CHARS): string {
  const logical: string[] = [];
  let length = 0;
  for (const line of lines) {
    const raw = lineText(line);
    if (line.isWrapped && logical.length > 0) {
      logical[logical.length - 1] += raw;
    } else {
      logical.push(raw);
      if (logical.length > 1) length += 1;
    }
    length += raw.length;
    if (length > maxChars) throw new CommandBlockTextLimitError(maxChars);
  }
  const text = logical.map((part) => part.trimEnd()).join("\n");
  if (text.length > maxChars) throw new CommandBlockTextLimitError(maxChars);
  return text;
}

export function resolveCommandBlockRange(
  terminal: Terminal,
  block: CommandBlock,
): { startLine: number; endLine: number } | null {
  if (block.start.isDisposed || block.start.line < 0) return null;
  const buffer = terminal.buffer.normal;
  const cursorLine = buffer.baseY + buffer.cursorY;
  const endLine = block.end && !block.end.isDisposed ? block.end.line : cursorLine;
  return endLine < block.start.line ? null : { startLine: block.start.line, endLine };
}

export function extractCommandBlockText(
  terminal: Terminal,
  block: CommandBlock,
  maxChars = COMMAND_BLOCK_COPY_MAX_CHARS,
): string {
  const range = resolveCommandBlockRange(terminal, block);
  if (!range) return "";
  const lines: IBufferLine[] = [];
  const buffer = terminal.buffer.normal;
  for (let line = range.startLine; line <= range.endLine; line++) {
    const value = buffer.getLine(line);
    if (value) lines.push(value);
  }
  return linesToLogicalText(lines, maxChars);
}

export function commandBlockMetadata(
  terminal: Terminal,
  host: string,
  block: CommandBlock,
): CommandBlockMetadata | null {
  const range = resolveCommandBlockRange(terminal, block);
  if (!range) return null;
  return {
    id: block.id,
    host,
    command: block.command,
    color: block.color,
    startLine: range.startLine,
    endLine: range.endLine,
    startedAt: block.startedAt,
    endedAt: block.endedAt,
    active: block.end === null,
  };
}
