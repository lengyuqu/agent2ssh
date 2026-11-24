import { useState } from "react";
import { cn } from "../../lib/utils";

type HostChipListProps = {
  names: string[];
  onSelect?: (name: string) => void;
  /**
   * Names past this count stay behind a +N control until it is opened.
   * Omit it to show every name.
   */
  collapseAfter?: number;
  className?: string;
};

const chipCls =
  "inline-flex items-center rounded-full border border-border bg-muted px-2 py-0.5 text-xs font-medium text-foreground transition-colors hover:bg-primary/12 hover:text-primary";

/** Host names as chips. A click selects that host when the caller handles it. */
export function HostChipList({ names, onSelect, collapseAfter, className }: HostChipListProps) {
  const [expanded, setExpanded] = useState(false);
  if (names.length === 0) return null;
  const limit = collapseAfter ?? names.length;
  const hidden = !expanded && names.length > limit ? names.length - limit : 0;
  const shown = hidden > 0 ? names.slice(0, limit) : names;

  return (
    <div className={cn("flex flex-wrap items-center gap-1.5", className)}>
      {shown.map((name) =>
        onSelect ? (
          <button key={name} type="button" className={chipCls} onClick={() => onSelect(name)}>
            {name}
          </button>
        ) : (
          <span key={name} className={chipCls}>
            {name}
          </span>
        ),
      )}
      {hidden > 0 && (
        <button type="button" className={chipCls} onClick={() => setExpanded(true)}>
          +{hidden}
        </button>
      )}
    </div>
  );
}
