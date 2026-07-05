import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { api } from "./api";
import type { AgentEvent } from "./types";

export type EventsStatus = "connecting" | "live" | "offline";

type Listener = (event: AgentEvent) => void;

type EventsContextValue = {
  status: EventsStatus;
  subscribe: (listener: Listener) => () => void;
};

const EventsContext = createContext<EventsContextValue | null>(null);

const RECONNECT_MS = 3000;
const CONNECT_TIMEOUT_MS = 6000;

/**
 * V2-1: single shared connection to the daemon SSE event stream. Anything that
 * needs live agent events (LiveActivityPanel, NotificationCenter) subscribes
 * here instead of each opening its own `api.subscribeEvents` stream.
 */
export function EventsProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<EventsStatus>("connecting");
  const listenersRef = useRef<Set<Listener>>(new Set());

  useEffect(() => {
    let active = true;
    let controller: AbortController | null = null;
    let retryTimer: number | null = null;
    let connectTimer: number | null = null;

    function clearConnectTimer() {
      if (connectTimer !== null) {
        window.clearTimeout(connectTimer);
        connectTimer = null;
      }
    }

    function scheduleReconnect() {
      if (!active || retryTimer !== null) return;
      retryTimer = window.setTimeout(() => {
        retryTimer = null;
        connect();
      }, RECONNECT_MS);
    }

    function connect() {
      controller?.abort();
      controller = new AbortController();
      setStatus("connecting");

      connectTimer = window.setTimeout(() => {
        controller?.abort();
      }, CONNECT_TIMEOUT_MS);

      api
        .subscribeEvents(
          (event) => {
            clearConnectTimer();
            setStatus("live");
            for (const listener of listenersRef.current) listener(event);
          },
          controller.signal,
          () => {
            clearConnectTimer();
            setStatus("live");
          }
        )
        .then(() => {
          // The stream ended without an error (e.g. the daemon restarted and
          // closed the connection cleanly). Treat it like a drop: go offline
          // and reconnect, otherwise the bus would stay "live" but dead.
          clearConnectTimer();
          if (!active) return;
          setStatus("offline");
          scheduleReconnect();
        })
        .catch((err) => {
          clearConnectTimer();
          if (!active) return;
          if (controller?.signal.aborted) {
            api
              .writeDiagnosticLog("warn", "events-bus", "event stream connect timed out or was aborted")
              .catch(() => {});
            scheduleReconnect();
            return;
          }
          setStatus("offline");
          api
            .writeDiagnosticLog("error", "events-bus", "event stream subscription failed", {
              error: String(err),
            })
            .catch(() => {});
          scheduleReconnect();
        });
    }

    connect();
    return () => {
      active = false;
      controller?.abort();
      clearConnectTimer();
      if (retryTimer !== null) window.clearTimeout(retryTimer);
    };
  }, []);

  const subscribe = useCallback((listener: Listener) => {
    listenersRef.current.add(listener);
    return () => listenersRef.current.delete(listener);
  }, []);

  const value = useMemo<EventsContextValue>(() => ({ status, subscribe }), [status, subscribe]);

  return <EventsContext.Provider value={value}>{children}</EventsContext.Provider>;
}

function useEventsContext(): EventsContextValue {
  const ctx = useContext(EventsContext);
  if (!ctx) throw new Error("useEventsBus hooks must be used within EventsProvider");
  return ctx;
}

export function useEventsStatus(): EventsStatus {
  return useEventsContext().status;
}

/** Subscribe to every agent event while the calling component is mounted. */
export function useAgentEvents(listener: Listener): void {
  const { subscribe } = useEventsContext();
  const listenerRef = useRef(listener);
  listenerRef.current = listener;
  useEffect(() => subscribe((event) => listenerRef.current(event)), [subscribe]);
}
