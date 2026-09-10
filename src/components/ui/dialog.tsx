import { useCallback, useEffect, useRef, useState } from "react";
import { cn } from "../../lib/utils";
import { Button } from "./button";

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
          "w-full max-w-md rounded-xl border border-edge bg-popover p-6 text-popover-foreground",
          className
        )}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}

export type ConfirmOptions = {
  title: string;
  description?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
};

type PendingConfirm = ConfirmOptions & { resolve: (ok: boolean) => void };

type ConfirmDialogProps = ConfirmOptions & {
  onConfirm: () => void;
  onCancel: () => void;
};

/** Controlled confirmation dialog built on <Dialog>. */
export function ConfirmDialog({
  title,
  description,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  danger = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Dialog onClose={onCancel} className="max-w-sm">
      <p className="text-sm font-medium text-foreground">{title}</p>
      {description && <p className="mt-2 text-xs text-muted-foreground">{description}</p>}
      <div className="mt-4 flex justify-end gap-2.5">
        <Button variant="secondary" size="sm" onClick={onCancel}>
          {cancelLabel}
        </Button>
        <Button variant={danger ? "destructive" : "default"} size="sm" onClick={onConfirm}>
          {confirmLabel}
        </Button>
      </div>
    </Dialog>
  );
}

let confirmListener: ((pending: PendingConfirm | null) => void) | null = null;

/**
 * Imperative async confirmation. Resolves true when confirmed, false when
 * cancelled (or when no <ConfirmHost /> is mounted). Replaces window.confirm
 * with app-styled, i18n-able copy.
 */
export function confirmDialog(options: ConfirmOptions): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    if (!confirmListener) {
      resolve(false);
      return;
    }
    confirmListener({ ...options, resolve });
  });
}

/** Single global host for confirmDialog(). Mount once near the app root. */
export function ConfirmHost() {
  const [pending, setPending] = useState<PendingConfirm | null>(null);
  const resolverRef = useRef<((ok: boolean) => void) | null>(null);

  useEffect(() => {
    confirmListener = (next) => {
      if (!next) return;
      resolverRef.current = next.resolve;
      setPending(next);
    };
    return () => {
      confirmListener = null;
    };
  }, []);

  const settle = useCallback((ok: boolean) => {
    resolverRef.current?.(ok);
    resolverRef.current = null;
    setPending(null);
  }, []);

  if (!pending) return null;

  return (
    <ConfirmDialog
      title={pending.title}
      description={pending.description}
      confirmLabel={pending.confirmLabel}
      cancelLabel={pending.cancelLabel}
      danger={pending.danger}
      onConfirm={() => settle(true)}
      onCancel={() => settle(false)}
    />
  );
}
