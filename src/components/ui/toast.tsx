import { AlertTriangle, CheckCircle2, X, XCircle } from "lucide-react";
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

export type ToastVariant = "success" | "error" | "warning";

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

const AUTO_DISMISS_MS = 5000;

const VARIANT_STYLES: Record<ToastVariant, string> = {
  success: "border-success/40 text-success",
  error: "border-destructive/40 text-destructive",
  warning: "border-warning/40 text-warning",
};

const VARIANT_ICON: Record<ToastVariant, typeof CheckCircle2> = {
  success: CheckCircle2,
  error: XCircle,
  warning: AlertTriangle,
};

/** V1-4: app-wide toast host. Wrap the tree once (see main.tsx); call useToast() anywhere below it. */
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
      const duration = options?.durationMs === undefined ? AUTO_DISMISS_MS : options.durationMs;
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

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="pointer-events-none fixed inset-x-0 bottom-10 z-[1300] flex flex-col items-center gap-2 px-4">
        {toasts.map((item) => {
          const Icon = VARIANT_ICON[item.variant];
          return (
            <div
              key={item.id}
              role="status"
              className={cn(
                "pointer-events-auto grid w-full max-w-md gap-1.5 rounded-lg border bg-card/95 px-3 py-2 text-sm text-foreground shadow-lg backdrop-blur-sm",
                VARIANT_STYLES[item.variant]
              )}
            >
              <div className="flex items-start gap-2">
                <Icon size={16} className="mt-0.5 shrink-0" />
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
