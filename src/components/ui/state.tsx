import { AlertCircle, Inbox, Loader2, type LucideIcon } from "lucide-react";
import { Button } from "./button";
import { cn } from "../../lib/utils";

type StateAction = {
  label: string;
  onClick: () => void;
};

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
      <Loader2 size={20} className="animate-spin" />
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
