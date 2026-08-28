import { AlertTriangle, CheckCircle2, Loader2, X, XCircle } from "lucide-react";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react";
import { cn } from "../../lib/utils";
import { Button } from "./button";

export type ToastVariant = "success" | "error" | "warning" | "progress";

export type ToastAction = {
  label: string;
  onClick: () => void;
};

export type ToastOptions = {
  title?: string;
  actions?: ToastAction[];
  /** ms until auto-dismiss; `null` means sticky (manual dismiss / actions only). */
  durationMs?: number | null;
};

type ToastItem = {
  id: number;
  variant: ToastVariant;
  message: string;
  title?: string;
  actions?: ToastAction[];
};

type ToastContextValue = {
  /** Returns the toast id so callers can `dismissToast` it later (e.g. once the
   * underlying event it represents — an approval, say — resolves elsewhere). */
  showToast: (variant: ToastVariant, message: string, options?: ToastOptions) => number;
  dismissToast: (id: number) => void;
};

const ToastContext = createContext<ToastContextValue | null>(null);

// v3 §3.6: toasts fade after 3.5s; errors stay until manually dismissed.
const AUTO_DISMISS_MS = 3500;
// Right-top stack shows at most 3 toasts; older ones collapse into a +N pill.
const VISIBLE_CAP = 3;

/* v3: the semantic color lives in a 3px left edge bar (risk families + action
   cyan for in-flight background tasks), on a canvas-3 surface with an edge
   border — no shadows. */
const VARIANT_EDGE: Record<ToastVariant, string> = {
  success: "bg-risk-low",
  error: "bg-risk-high",
  warning: "bg-risk-medium",
  // Action-color discipline: cyan marks a clickable/in-flight activity, not a state.
  progress: "bg-primary",
};

const VARIANT_ICON_CLS: Record<ToastVariant, string> = {
  success: "text-risk-low",
  error: "text-risk-high",
  warning: "text-risk-medium",
  progress: "text-primary animate-spin",
};

const VARIANT_ICON: Record<ToastVariant, typeof CheckCircle2> = {
  success: CheckCircle2,
  error: XCircle,
  warning: AlertTriangle,
  progress: Loader2,
};

/** V1-4: app-wide toast host. Wrap the tree once (see main.tsx); call useToast() anywhere below it.
 *  v3 §3.6: right-top stack (≤3 visible, overflow collapsed as +N), canvas-3
 *  surface with a 3px semantic left edge, 3.5s auto-dismiss, sticky errors. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const idRef = useRef(0);
  const timersRef = useRef(new Map<number, number>());

  const dismiss = useCallback((id: number) => {
    setToasts((prev) => prev.filter((item) => item.id !== id));
    const timer = timersRef.current.get(id);
    if (timer !== undefined) {
      window.clearTimeout(timer);
      timersRef.current.delete(id);
    }
  }, []);

  const showToast = useCallback(
    (variant: ToastVariant, message: string, options?: ToastOptions) => {
      const id = ++idRef.current;
      setToasts((prev) => [...prev, { id, variant, message, title: options?.title, actions: options?.actions }]);
      // Explicit duration wins; otherwise errors are sticky and everything
      // else auto-dismisses on the 3.5s clock.
      const duration =
        options?.durationMs !== undefined
          ? options.durationMs
          : variant === "error"
            ? null
            : AUTO_DISMISS_MS;
      if (duration !== null) {
        const timer = window.setTimeout(() => dismiss(id), duration);
        timersRef.current.set(id, timer);
      }
      return id;
    },
    [dismiss]
  );

  const value = useMemo<ToastContextValue>(
    () => ({ showToast, dismissToast: dismiss }),
    [showToast, dismiss]
  );

  const visible = toasts.slice(-VISIBLE_CAP);
  const hiddenCount = toasts.length - visible.length;
  // Newest on top of the right-top stack.
  const ordered = [...visible].reverse();

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="pointer-events-none fixed inset-x-0 top-12 z-[1300] flex flex-col items-end gap-2 px-4">
        {hiddenCount > 0 && (
          <div className="pointer-events-none rounded-full border border-line bg-canvas-2 px-2.5 py-0.5 font-mono text-[10px] text-faint">
            +{hiddenCount}
          </div>
        )}
        {ordered.map((item) => {
          const Icon = VARIANT_ICON[item.variant];
          return (
            <div
              key={item.id}
              role="status"
              className="toast-enter pointer-events-auto relative grid w-full max-w-md gap-1.5 overflow-hidden rounded-lg border border-edge bg-popover px-3 py-2 pl-4 text-sm text-foreground"
            >
              <span
                className={cn("absolute inset-y-0 left-0 w-[3px]", VARIANT_EDGE[item.variant])}
                aria-hidden
              />
              <div className="flex items-start gap-2">
                <Icon size={16} className={cn("mt-0.5 shrink-0", VARIANT_ICON_CLS[item.variant])} />
                <div className="min-w-0 flex-1">
                  {item.title && <div className="font-semibold">{item.title}</div>}
                  <span className="break-words text-foreground/90">{item.message}</span>
                </div>
                <button
                  type="button"
                  className="shrink-0 rounded p-0.5 text-muted-foreground hover:text-foreground"
                  onClick={() => dismiss(item.id)}
                >
                  <X size={13} />
                </button>
              </div>
              {item.actions && item.actions.length > 0 && (
                <div className="flex justify-end gap-1.5 pl-6">
                  {item.actions.map((action) => (
                    <Button
                      key={action.label}
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        action.onClick();
                        dismiss(item.id);
                      }}
                    >
                      {action.label}
                    </Button>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used within ToastProvider");
  return ctx;
}
