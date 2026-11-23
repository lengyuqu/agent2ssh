import { createPortal } from "react-dom";
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";

type Registry = {
  register: (id: string, node: HTMLElement | null) => void;
  node: (id: string) => HTMLElement | null;
};

const DockContext = createContext<Registry | null>(null);

/**
 * One TerminalView, two places it can sit: the approval/audit drawer, then the
 * main terminal pane. The view stays mounted and is portaled into whichever
 * slot is registered, so the daemon WebSocket (the PTY) is not opened again.
 */
export function TerminalDock({ children }: { children: ReactNode }) {
  const nodes = useState(() => new Map<string, HTMLElement>())[0];
  const [, bump] = useState(0);
  const register = useCallback(
    (id: string, node: HTMLElement | null) => {
      const current = nodes.get(id) ?? null;
      if (current === node) return;
      if (node) nodes.set(id, node);
      else nodes.delete(id);
      bump((value) => value + 1);
    },
    [nodes],
  );
  const node = useCallback((id: string) => nodes.get(id) ?? null, [nodes]);
  const value = useMemo(() => ({ register, node }), [register, node]);
  return <DockContext.Provider value={value}>{children}</DockContext.Provider>;
}

export function TerminalSlot({ id, className }: { id: string; className?: string }) {
  const dock = useContext(DockContext);
  const ref = useCallback(
    (element: HTMLDivElement | null) => dock?.register(id, element),
    [dock, id],
  );
  return <div ref={ref} className={className ?? "h-full min-h-0"} data-terminal-slot={id} />;
}

export function DockedView({ slotId, children }: { slotId: string; children: ReactNode }) {
  const dock = useContext(DockContext);
  const [park, setPark] = useState<HTMLDivElement | null>(null);
  const target = dock?.node(slotId) ?? park;
  return (
    <>
      <div
        ref={setPark}
        className="pointer-events-none fixed left-0 top-0 -z-10 h-px w-px overflow-hidden"
        aria-hidden
      />
      {target ? createPortal(children, target) : null}
    </>
  );
}
