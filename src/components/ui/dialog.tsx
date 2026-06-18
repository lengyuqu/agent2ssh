import { cn } from "../../lib/utils";

type DialogProps = {
  onClose?: () => void;
  className?: string;
  children: React.ReactNode;
};

/** Lightweight modal overlay + centered card. Click-outside calls onClose. */
export function Dialog({ onClose, className, children }: DialogProps) {
  return (
    <div
      className="fixed inset-0 z-[1000] flex items-center justify-center bg-black/50 p-4"
      onClick={onClose}
    >
      <div
        className={cn(
          "w-full max-w-md rounded-xl border border-border bg-card p-6 text-card-foreground shadow-2xl",
          className
        )}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
