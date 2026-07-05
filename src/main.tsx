import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import ErrorBoundary from "./components/ErrorBoundary";
import { logDiagnostic } from "./api";
import { EventsProvider } from "./eventsBus";
import { I18nProvider } from "./i18n";
import { ThemeProvider } from "./theme";
import { ToastProvider } from "./components/ui/toast";
import "./index.css";

// Global capture for uncaught errors and unhandled promise rejections that
// escape React's render tree (event handlers, async callbacks, timers, …).
// These would otherwise only reach the devtools console; forwarding them to the
// backend diagnostic log makes frontend faults observable in app.log.
window.addEventListener("error", (event) => {
  const error = event.error;
  logDiagnostic("error", "frontend", "uncaught error", {
    message: event.message,
    source: event.filename,
    line: event.lineno,
    column: event.colno,
    stack: error instanceof Error ? error.stack : undefined,
  });
});

window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  logDiagnostic("error", "frontend", "unhandled promise rejection", {
    error: reason instanceof Error ? reason.message : String(reason),
    stack: reason instanceof Error ? reason.stack : undefined,
  });
});

createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <I18nProvider>
        <ToastProvider>
          <EventsProvider>
            <ErrorBoundary>
              <App />
            </ErrorBoundary>
          </EventsProvider>
        </ToastProvider>
      </I18nProvider>
    </ThemeProvider>
  </React.StrictMode>
);
