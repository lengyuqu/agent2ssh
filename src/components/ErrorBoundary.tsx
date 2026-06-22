import { Component, type ErrorInfo, type ReactNode } from "react";
import { reportError } from "../api";
import { useI18n } from "../i18n";

type Props = {
  children: ReactNode;
  t: (text: string) => string;
};

type State = {
  error: Error | null;
};

/**
 * Top-level React error boundary. Catches render/lifecycle exceptions that would
 * otherwise blank the whole app, persists them to the backend diagnostic log,
 * and shows a minimal recovery screen. Paired with the global `window.onerror`
 * / `unhandledrejection` handlers installed in `main.tsx`, this ensures frontend
 * crashes are observable in `app.log` instead of vanishing silently.
 */
class ErrorBoundaryInner extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    reportError("frontend", "react render error", error, {
      componentStack: info.componentStack ?? undefined,
    });
  }

  handleReload = (): void => {
    this.setState({ error: null });
    window.location.reload();
  };

  render(): ReactNode {
    const { error } = this.state;
    if (!error) {
      return this.props.children;
    }

    return (
      <div className="flex min-h-screen flex-col items-center justify-center gap-4 bg-background p-8 text-center">
        <h1 className="text-lg font-semibold text-destructive">
          {this.props.t("Something went wrong")}
        </h1>
        <p className="max-w-md text-sm text-muted-foreground">
          {this.props.t(
            "The interface hit an unexpected error and stopped rendering. The details were written to the diagnostic log (Settings → Diagnostics)."
          )}
        </p>
        <pre className="max-h-40 max-w-full overflow-auto rounded-md bg-[#0e1620] px-3 py-2 text-left text-xs text-[#e6edf3]">
          {error.message}
        </pre>
        <button
          type="button"
          onClick={this.handleReload}
          className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground"
        >
          {this.props.t("Reload")}
        </button>
      </div>
    );
  }
}

export default function ErrorBoundary({ children }: { children: ReactNode }) {
  const { t } = useI18n();
  return <ErrorBoundaryInner t={t}>{children}</ErrorBoundaryInner>;
}
