import { ChevronDown, ChevronUp, ChevronsUpDown, Columns3 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { Table } from "@tanstack/react-table";
import { IconButton } from "./icon-button";

/** V3-3: small shared bits for TanStack-Table-backed lists (HostList, AuditPanel). */
export function SortIcon({ direction }: { direction: false | "asc" | "desc" }) {
  if (direction === "asc") return <ChevronUp size={12} />;
  if (direction === "desc") return <ChevronDown size={12} />;
  return <ChevronsUpDown size={12} className="opacity-40" />;
}

export function ColumnVisibilityMenu<T>({ table, label }: { table: Table<T>; label: string }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onPointerDown(event: PointerEvent) {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  const hideable = table.getAllLeafColumns().filter((column) => column.getCanHide());
  if (hideable.length === 0) return null;

  return (
    <div className="relative" ref={ref}>
      <IconButton title={label} onClick={() => setOpen((value) => !value)}>
        <Columns3 size={15} />
      </IconButton>
      {open && (
        <div className="absolute right-0 top-[calc(100%+6px)] z-30 grid w-48 gap-0.5 rounded-lg border border-border bg-popover p-2 text-popover-foreground shadow-xl">
          {hideable.map((column) => (
            <label
              key={column.id}
              className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-sm hover:bg-muted"
            >
              <input
                type="checkbox"
                className="size-3.5 accent-primary"
                checked={column.getIsVisible()}
                onChange={column.getToggleVisibilityHandler()}
              />
              {typeof column.columnDef.header === "string" ? column.columnDef.header : column.id}
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
