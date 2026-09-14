import type { ReactNode } from "react";
import { AlertCircle, Inbox, Loader2, type LucideIcon } from "lucide-react";
import { Button } from "./button";
import { cn } from "../../lib/utils";

type StateAction = {
  label: string;
  onClick: () => void;
};

type SpinnerProps = {
  size?: number;
  className?: string;
};

/**
 * The one spinner mark.
 *
 * Panels used to write `<Loader2 className="animate-spin" />` by hand — twelve
 * times, at four different sizes — so the glyph was decided in twelve places.
 * Anything that spins should go through this, which also makes the icon
 * decorative for assistive tech instead of an unnamed graphic.
 */
export function Spinner({ size = 14, className }: SpinnerProps) {
  return <Loader2 size={size} aria-hidden className={cn("animate-spin", className)} />;
}

type EmptyStateProps = {
  icon?: LucideIcon;
  title: string;
  description?: string;
  action?: StateAction;
  className?: string;
};

/** V1-5: shared empty/loading/error placeholders — consistent icon + copy + action across panels. */
export function EmptyState({ icon: Icon = Inbox, title, description, action, className }: EmptyStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center gap-2 rounded-lg border border-dashed border-border px-6 py-10 text-center",
        className
      )}
    >
      <Icon size={22} className="text-muted-foreground/60" />
      <div className="text-sm font-medium text-foreground">{title}</div>
      {description && <div className="max-w-sm text-xs text-muted-foreground">{description}</div>}
      {action && (
        <Button variant="secondary" size="sm" className="mt-2" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
    </div>
  );
}

type LoadingStateProps = {
  label?: string;
  className?: string;
};

export function LoadingState({ label, className }: LoadingStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center gap-2 rounded-lg px-6 py-10 text-center text-muted-foreground",
        className
      )}
    >
      <Spinner size={20} />
      {label && <div className="text-xs">{label}</div>}
    </div>
  );
}

type ErrorStateProps = {
  message: string;
  action?: StateAction;
  className?: string;
};

export function ErrorState({ message, action, className }: ErrorStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-6 py-8 text-center",
        className
      )}
    >
      <AlertCircle size={20} className="text-destructive" />
      <div className="max-w-sm text-sm text-destructive">{message}</div>
      {action && (
        <Button variant="secondary" size="sm" className="mt-2" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
    </div>
  );
}

type InlineAlertTone = "warning" | "destructive";

// Tailwind builds its class list at compile time, so `bg-${tone}/10` would
// never be generated. Each tone is spelled out.
const INLINE_ALERT_TONES: Record<InlineAlertTone, { plain: string; bordered: string }> = {
  warning: {
    plain: "bg-warning/10 text-warning",
    bordered: "border-warning/30 bg-warning/10 text-warning",
  },
  destructive: {
    plain: "bg-destructive/10 text-destructive",
    bordered: "border-destructive/30 bg-destructive/10 text-destructive",
  },
};

type InlineAlertProps = {
  tone?: InlineAlertTone;
  icon?: LucideIcon;
  /** Adds a border and slightly wider padding. Use for standalone alerts, not for notes in a list. */
  bordered?: boolean;
  children: ReactNode;
  className?: string;
};

/**
 * An alert or warning note *inside* a panel or dialog.
 *
 * This is the counterpart to <ErrorState>, which is the full-panel placeholder.
 * Ten call sites had hand-rolled this box and drifted apart: three different
 * paddings, two border alphas, two font sizes. The warning note alone was copied
 * verbatim six times, so this is the single spec for both shapes.
 */
export function InlineAlert({
  tone = "warning",
  icon: Icon,
  bordered = false,
  children,
  className,
}: InlineAlertProps) {
  const spec = INLINE_ALERT_TONES[tone];
  return (
    <div
      className={cn(
        "rounded-md text-sm",
        bordered ? cn("border px-3 py-2", spec.bordered) : cn("px-2.5 py-2", spec.plain),
        Icon && "flex items-start gap-2",
        className
      )}
    >
      {Icon && <Icon size={15} className="mt-0.5 shrink-0" />}
      <div className="break-words">{children}</div>
    </div>
  );
}
