import type { IDecoration, IMarker, Terminal } from "@xterm/xterm";
import type { CompiledHighlightRule } from "./highlight";
import { planLine } from "./highlight";

type LineDecorations = {
  signature: string;
  marker: IMarker;
  decorations: IDecoration[];
};

/**
 * Applies highlight rules to the visible xterm buffer without changing the
 * PTY byte stream. Decorations are rebuilt only when a physical buffer line
 * changes, and are also refreshed when the user scrolls into scrollback.
 */
export class TerminalHighlightDecorations {
  private rules: CompiledHighlightRule[];
  private lines = new Map<number, LineDecorations>();
  private alternateOverlay: HTMLDivElement | null = null;

  constructor(
    private readonly terminal: Terminal,
    rules: CompiledHighlightRule[],
  ) {
    this.rules = rules;
  }

  setRules(rules: CompiledHighlightRule[]): void {
    this.rules = rules;
    this.clear();
    this.refresh();
  }

  refresh(): void {
    const buffer = this.terminal.buffer.active;
    if (buffer.type === "alternate") {
      this.renderAlternateBuffer();
      return;
    }
    this.clearAlternateOverlay();
    this.reindexNormalBufferLines();
    const cursorLine = buffer.baseY + buffer.cursorY;
    const firstLine = buffer.viewportY;
    const lastLine = Math.min(buffer.length - 1, firstLine + this.terminal.rows - 1);

    for (let lineIndex = firstLine; lineIndex <= lastLine; lineIndex++) {
      const text = buffer.getLine(lineIndex)?.translateToString(true) ?? "";
      const planned = planLine(text, this.rules);
      const signature = `${text}\u0000${planned
        .map((item) => `${item.x}:${item.width}:${item.color}`)
        .join("|")}`;
      const existing = this.lines.get(lineIndex);
      if (existing?.signature === signature) continue;
      if (existing) this.disposeLine(lineIndex, existing);
      if (planned.length === 0) continue;

      const marker = this.terminal.registerMarker(lineIndex - cursorLine);
      const decorations: IDecoration[] = [];
      for (const item of planned) {
        if (item.width <= 0) continue;
        const decoration = this.terminal.registerDecoration({
          marker,
          x: item.x,
          width: item.width,
          foregroundColor: item.color,
          layer: "top",
        });
        if (decoration) decorations.push(decoration);
      }
      if (decorations.length === 0) {
        marker.dispose();
        continue;
      }
      const entry = { signature, marker, decorations };
      marker.onDispose(() => {
        if (this.lines.get(lineIndex) === entry) this.lines.delete(lineIndex);
      });
      this.lines.set(lineIndex, entry);
    }
  }

  dispose(): void {
    this.clear();
  }

  private clear(): void {
    this.clearAlternateOverlay();
    for (const [lineIndex, entry] of [...this.lines]) {
      this.disposeLine(lineIndex, entry);
    }
  }

  private reindexNormalBufferLines(): void {
    const reindexed = new Map<number, LineDecorations>();
    for (const entry of this.lines.values()) {
      if (entry.marker.isDisposed) {
        for (const decoration of entry.decorations) decoration.dispose();
        continue;
      }
      reindexed.set(entry.marker.line, entry);
    }
    this.lines = reindexed;
  }

  /**
   * xterm deliberately does not create markers/decorations in the alternate
   * buffer. TUIs use that buffer, so render a cell-aligned, pointer-transparent
   * overlay there instead of silently dropping configured highlights.
   */
  private renderAlternateBuffer(): void {
    const screen = this.terminal.element?.querySelector<HTMLElement>(".xterm-screen");
    if (!screen || this.terminal.cols <= 0 || this.terminal.rows <= 0) return;

    if (!this.alternateOverlay) {
      const overlay = document.createElement("div");
      overlay.setAttribute("aria-hidden", "true");
      Object.assign(overlay.style, {
        position: "absolute",
        inset: "0",
        pointerEvents: "none",
        zIndex: "5",
        overflow: "hidden",
      });
      screen.appendChild(overlay);
      this.alternateOverlay = overlay;
    }
    this.alternateOverlay.replaceChildren();

    const buffer = this.terminal.buffer.active;
    const rect = screen.getBoundingClientRect();
    const cellWidth = rect.width / this.terminal.cols;
    const cellHeight = rect.height / this.terminal.rows;
    const firstLine = buffer.viewportY;
    const lastLine = Math.min(buffer.length - 1, firstLine + this.terminal.rows - 1);

    for (let lineIndex = firstLine; lineIndex <= lastLine; lineIndex++) {
      const text = buffer.getLine(lineIndex)?.translateToString(true) ?? "";
      for (const item of planLine(text, this.rules)) {
        if (item.width <= 0) continue;
        const highlight = document.createElement("span");
        Object.assign(highlight.style, {
          position: "absolute",
          left: `${item.x * cellWidth}px`,
          top: `${(lineIndex - firstLine) * cellHeight}px`,
          width: `${item.width * cellWidth}px`,
          height: `${cellHeight}px`,
          backgroundColor: `${item.color}26`,
          boxShadow: `inset 0 -2px ${item.color}`,
        });
        this.alternateOverlay.appendChild(highlight);
      }
    }
  }

  private clearAlternateOverlay(): void {
    this.alternateOverlay?.remove();
    this.alternateOverlay = null;
  }

  private disposeLine(lineIndex: number, entry: LineDecorations): void {
    this.lines.delete(lineIndex);
    for (const decoration of entry.decorations) decoration.dispose();
    if (!entry.marker.isDisposed) entry.marker.dispose();
  }
}
