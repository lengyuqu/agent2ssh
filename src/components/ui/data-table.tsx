import { ChevronDown, ChevronUp, ChevronsUpDown, Columns3 } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { flexRender, type ColumnDef, type Row, type Table } from "@tanstack/react-table";
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
        <div className="absolute right-0 top-[calc(100%+6px)] z-30 grid w-48 gap-0.5 rounded-lg border border-edge bg-popover p-2 text-popover-foreground">
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

/** Shared row-selection display column: header select-all (indeterminate) + per-row checkbox. */
export function selectColumn<T>(
  options: { size?: number; stopRowClick?: boolean } = {}
): ColumnDef<T, unknown> {
  const { size = 28, stopRowClick = false } = options;
  return {
    id: "select",
    size,
    header: ({ table }) => (
      <input
        type="checkbox"
        className="size-4 accent-primary"
        checked={table.getIsAllRowsSelected()}
        ref={(el) => {
          if (el) el.indeterminate = table.getIsSomeRowsSelected() && !table.getIsAllRowsSelected();
        }}
        onChange={table.getToggleAllRowsSelectedHandler()}
      />
    ),
    cell: ({ row }) => (
      <input
        type="checkbox"
        className="size-4 accent-primary"
        checked={row.getIsSelected()}
        onChange={row.getToggleSelectedHandler()}
        onClick={stopRowClick ? (event) => event.stopPropagation() : undefined}
      />
    ),
    enableSorting: false,
    enableHiding: false,
  };
}

/** Shared sortable table skeleton (header row + body rows) used by HostList / AuditPanel. */
export function DataTable<T>({
  table,
  rows,
  minWidth = 560,
  wrapperClassName = "overflow-x-auto",
  rowClassName,
  onRowClick,
  footer,
}: {
  table: Table<T>;
  rows?: Row<T>[];
  minWidth?: number;
  wrapperClassName?: string;
  rowClassName?: (row: Row<T>) => string;
  onRowClick?: (row: Row<T>) => void;
  footer?: ReactNode;
}) {
  const bodyRows = rows ?? table.getRowModel().rows;
  return (
    <div className={wrapperClassName}>
      <table className="w-full border-collapse text-sm" style={{ minWidth }}>
        <thead>
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id} className="border-b border-border text-left">
              {headerGroup.headers.map((header) => (
                <th key={header.id} className="px-2 py-1.5 font-medium text-muted-foreground">
                  {header.isPlaceholder ? null : header.column.getCanSort() ? (
                    <button
                      type="button"
                      className="flex items-center gap-1 hover:text-foreground"
                      onClick={header.column.getToggleSortingHandler()}
                    >
                      {flexRender(header.column.columnDef.header, header.getContext())}
                      <SortIcon direction={header.column.getIsSorted()} />
                    </button>
                  ) : (
                    flexRender(header.column.columnDef.header, header.getContext())
                  )}
                </th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {bodyRows.map((row) => (
            <tr
              key={row.id}
              onClick={onRowClick ? () => onRowClick(row) : undefined}
              className={rowClassName ? rowClassName(row) : undefined}
            >
              {row.getVisibleCells().map((cell) => (
                <td key={cell.id} className="px-2 py-2 align-middle">
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {footer}
    </div>
  );
}
