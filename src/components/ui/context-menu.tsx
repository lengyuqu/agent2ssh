import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { cn } from "../../lib/utils";

export type ContextMenuItem = {
  /** Already-localized label text. */
  label: string;
  onSelect: () => void;
  /** Renders in the destructive color. Use for delete-style actions. */
  danger?: boolean;
  disabled?: boolean;
};

type ContextMenuProps = {
  /** Viewport coordinates of the originating click. */
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
};

/** Keeps the menu this far away from the viewport edges when clamping. */
const EDGE_MARGIN = 8;

/**
 * A right-click menu anchored at viewport coordinates.
 *
 * Renders a transparent full-screen backdrop so any outside click dismisses it,
 * clamps itself back inside the viewport (a right-click near the bottom-right
 * corner would otherwise open a menu that runs off-screen), and supports
 * Escape plus Up/Down/Home/End/Enter navigation — a menu that only responds to
 * the mouse is unusable without one.
 */
export function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement | null>(null);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [pos, setPos] = useState({ left: x, top: y });
  const [activeIndex, setActiveIndex] = useState(0);

  // Clamp into the viewport before paint, so the menu never flashes off-screen.
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const maxLeft = Math.max(EDGE_MARGIN, window.innerWidth - EDGE_MARGIN - width);
    const maxTop = Math.max(EDGE_MARGIN, window.innerHeight - EDGE_MARGIN - height);
    setPos({
      left: Math.min(Math.max(EDGE_MARGIN, x), maxLeft),
      top: Math.min(Math.max(EDGE_MARGIN, y), maxTop),
    });
  }, [x, y]);

  // Move focus into the menu so Escape and the arrow keys reach it immediately.
  useEffect(() => {
    const firstEnabled = items.findIndex((item) => !item.disabled);
    const index = firstEnabled === -1 ? 0 : firstEnabled;
    setActiveIndex(index);
    itemRefs.current[index]?.focus();
    // Only on mount: re-focusing on every `items` change would fight the user.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Step to the next enabled item, wrapping at both ends. */
  function move(delta: number) {
    if (items.length === 0) return;
    let next = activeIndex;
    for (let step = 0; step < items.length; step += 1) {
      next = (next + delta + items.length) % items.length;
      if (!items[next]?.disabled) break;
    }
    setActiveIndex(next);
    itemRefs.current[next]?.focus();
  }

  function focusEdge(fromEnd: boolean) {
    const order = fromEnd ? [...items.keys()].reverse() : [...items.keys()];
    const target = order.find((index) => !items[index]?.disabled);
    if (target === undefined) return;
    setActiveIndex(target);
    itemRefs.current[target]?.focus();
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        onClose();
        break;
      case "ArrowDown":
        event.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        event.preventDefault();
        move(-1);
        break;
      case "Home":
        event.preventDefault();
        focusEdge(false);
        break;
      case "End":
        event.preventDefault();
        focusEdge(true);
        break;
      default:
        break;
    }
  }

  return (
    <div
      className="fixed inset-0 z-[1200]"
      // A capture-phase dismiss so a click on any underlying control closes the
      // menu instead of also triggering that control.
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <div
        ref={menuRef}
        role="menu"
        aria-orientation="vertical"
        tabIndex={-1}
        style={{ left: pos.left, top: pos.top }}
        className={cn(
          "fixed min-w-[11rem] max-w-[18rem] rounded-lg border border-edge bg-popover p-1",
          "text-popover-foreground shadow-lg outline-none"
        )}
        onKeyDown={handleKeyDown}
      >
        {items.map((item, index) => (
          <button
            key={item.label}
            ref={(node) => {
              itemRefs.current[index] = node;
            }}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            tabIndex={-1}
            onMouseEnter={() => setActiveIndex(index)}
            onClick={() => {
              if (item.disabled) return;
              item.onSelect();
              onClose();
            }}
            className={cn(
              "flex w-full items-center rounded-md px-2.5 py-1.5 text-left text-sm font-medium transition-colors",
              "focus-visible:outline-none",
              item.disabled
                ? "cursor-not-allowed text-muted-foreground opacity-55"
                : "text-foreground hover:bg-muted focus-visible:bg-muted",
              item.danger && !item.disabled && "text-destructive hover:bg-destructive/10"
            )}
          >
            {item.label}
          </button>
        ))}
      </div>
    </div>
  );
}
