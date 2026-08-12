import { describe, expect, it } from "vitest";
import type { IBufferLine, IMarker, Terminal } from "@xterm/xterm";
import {
  CommandBlockTextLimitError,
  commandBlockMetadata,
  extractCommandBlockText,
  linesToLogicalText,
} from "./block-content";
import type { CommandBlock } from "./command-blocks";

type Cell = { text: string; width: number };

function line(cells: Cell[], wrapped = false): IBufferLine {
  return {
    isWrapped: wrapped,
    length: cells.length,
    getCell(index: number) {
      const cell = cells[index];
      if (!cell) return undefined;
      return {
        getChars: () => cell.text,
        getWidth: () => cell.width,
      };
    },
  } as unknown as IBufferLine;
}

function textLine(text: string, wrapped = false): IBufferLine {
  return line([...text].map((character) => ({ text: character, width: 1 })), wrapped);
}

function marker(value: number): IMarker {
  return { line: value, isDisposed: false } as IMarker;
}

function fixture(lines: IBufferLine[]) {
  const terminal = {
    buffer: {
      normal: {
        baseY: lines.length - 1,
        cursorY: 0,
        getLine: (index: number) => lines[index],
      },
    },
  } as unknown as Terminal;
  const block: CommandBlock = {
    id: "session:1",
    sequence: 1,
    color: "red",
    command: "echo hello",
    startedAt: "start",
    endedAt: "end",
    start: marker(0),
    end: marker(lines.length - 1),
  };
  return { terminal, block };
}

describe("command block text extraction", () => {
  it("joins soft wraps without a newline and trims only logical line endings", () => {
    const lines = [textLine("hello "), textLine("world  ", true), textLine("done   ")];
    expect(linesToLogicalText(lines)).toBe("hello world\ndone");
  });

  it("keeps wide glyphs once and removes terminal control characters", () => {
    const value = line([
      { text: "你", width: 2 },
      { text: "", width: 0 },
      { text: "\u001b", width: 1 },
      { text: "[31mred", width: 1 },
    ]);
    expect(linesToLogicalText([value])).toBe("你[31mred");
  });

  it("extracts an inclusive marker range from the normal buffer", () => {
    const { terminal, block } = fixture([textLine("$ echo hello"), textLine("hello")]);
    expect(extractCommandBlockText(terminal, block)).toBe("$ echo hello\nhello");
    expect(commandBlockMetadata(terminal, "prod", block)).toMatchObject({
      id: "session:1",
      host: "prod",
      command: "echo hello",
      startLine: 0,
      endLine: 1,
      active: false,
    });
  });

  it("fails closed before oversized content reaches the clipboard", () => {
    expect(() => linesToLogicalText([textLine("12345")], 4)).toThrow(CommandBlockTextLimitError);
  });

  it("skips disposed or inverted ranges", () => {
    const { terminal, block } = fixture([textLine("secret")]);
    (block.start as { isDisposed: boolean }).isDisposed = true;
    expect(extractCommandBlockText(terminal, block)).toBe("");
  });
});
