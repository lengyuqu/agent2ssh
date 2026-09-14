import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { cn } from "../../lib/utils";
import { Button } from "./button";

type DialogProps = {
  onClose?: () => void;
  className?: string;
  children: React.ReactNode;
};

/**
 * Lightweight modal overlay + centered card. Click-outside calls onClose.
 *
 * The card declares `bg-popover text-popover-foreground`, so its children
 * inherit the popover text token. What that does *not* mean: `--text-1 →
 * --foreground` is the design system's text token (docs/plans/ui-v3-midnight-ops.md)
 * and the `*-foreground` surface variables are shadcn-compat aliases of it, so
 * inside this card `text-foreground` is a no-op in Midnight Ops rather than an
 * override. Eleven sites on popover surfaces spell it out anyway — six in
 * dialogs (ConfirmDialog's title, two in App.tsx, FilePreview, AlgoPrefsDialog,
 * SnippetsDialog) and five on the palette, settings menu, context menu and
 * toast (which pairs `bg-popover` with `text-foreground` on one element) —
 * while all seven uses of `text-popover-foreground` sit on the `bg-popover`
 * container itself and never on a child. Only the two hand-tuned legacy themes
 * differentiate the tokens (`dark` #c7d0d8 vs #d1dae2, `nord` #d8dee9 vs
 * #eceff4). So don't "correct" one of those eleven in isolation: it would be
 * the only exception, and the edit would be invisible in the default theme and
 * unverifiable in the two it touches.
 */
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
  /**
   * Extra body rendered between the description and the buttons.
   *
   * This is where a consequence belongs — an `<InlineAlert>` saying that open
   * sessions become orphaned, not a `<p>`. The neutral elaboration (what the
   * action does, that it cannot be undone) stays in `description`, which is
   * quieter on purpose. Seven call sites used to hand-roll the dialog itself
   * and six of them put an alert here.
   */
  note?: React.ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  /** Keeps the confirm button disabled while the action is in flight. */
  confirmDisabled?: boolean;
};

type PendingConfirm = ConfirmOptions & { resolve: (ok: boolean) => void };

type ConfirmDialogProps = ConfirmOptions & {
  onConfirm: () => void;
  onCancel: () => void;
};

/**
 * Controlled confirmation dialog built on <Dialog>.
 *
 * This is the app's one spec for "are you sure?". The buttons are the default
 * size, matching <ApprovalDialog> and every other footer that ends with
 * `mt-4 flex justify-end gap-2.5`; a modal is the last place to shrink a
 * target, and `size="sm"` put the labels below the app's 14px base.
 *
 * The labels default through `t()` rather than to the literals "Confirm" and
 * "Cancel". Six of the seven `confirmDialog()` call sites took the literal
 * default and so shipped an untranslated Cancel button to every non-English
 * locale; the default now cannot be forgotten.
 *
 * The title writes `text-foreground` instead of inheriting the card's
 * `popover-foreground`. That is the repo-wide convention for full-strength
 * text on a popover surface, not an oversight — read <Dialog> above before
 * changing it.
 */
export function ConfirmDialog({
  title,
  description,
  note,
  confirmLabel,
  cancelLabel,
  danger = false,
  confirmDisabled = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useI18n();
  return (
    <Dialog onClose={onCancel} className="max-w-sm">
      <p className="text-sm font-medium text-foreground">{title}</p>
      {description && <p className="mt-2 text-xs text-muted-foreground">{description}</p>}
      {note && <div className="mt-2">{note}</div>}
      <div className="mt-4 flex justify-end gap-2.5">
        <Button variant="secondary" onClick={onCancel}>
          {cancelLabel ?? t("Cancel")}
        </Button>
        <Button
          variant={danger ? "destructive" : "default"}
          onClick={onConfirm}
          disabled={confirmDisabled}
        >
          {confirmLabel ?? t("Confirm")}
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
      note={pending.note}
      confirmLabel={pending.confirmLabel}
      cancelLabel={pending.cancelLabel}
      danger={pending.danger}
      confirmDisabled={pending.confirmDisabled}
      onConfirm={() => settle(true)}
      onCancel={() => settle(false)}
    />
  );
}
