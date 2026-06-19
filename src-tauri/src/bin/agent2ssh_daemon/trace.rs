tokio::task_local! {
    /// Per-request correlation id, set by `trace_id_middleware` for the duration
    /// of each HTTP request. Read synchronously inside `on_event`, so daemon
    /// `tracing` warnings/errors carry the same id that the response echoes back.
    static REQUEST_TRACE_ID: String;
}

const TRACE_ID_HEADER: &str = "x-agent2ssh-trace-id";

/// Axum middleware that binds a correlation id to each request: it reuses an
/// inbound `X-Agent2SSH-Trace-Id` header when a caller supplies one (so MCP /
/// desktop requests stay linked to the originating operation) or mints a fresh
/// one, runs the request inside that task-local scope, and echoes the id on the
/// response so clients can record it.
pub(super) async fn trace_id_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let trace_id = req
        .headers()
        .get(TRACE_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let echo = trace_id.clone();
    let mut response = REQUEST_TRACE_ID.scope(trace_id, next.run(req)).await;
    if let Ok(value) = axum::http::HeaderValue::from_str(&echo) {
        response.headers_mut().insert(TRACE_ID_HEADER, value);
    }
    response
}

/// Collects an event's `message` and remaining fields into a JSON-ready shape so
/// the diagnostic bridge can persist them.
#[derive(Default)]
struct DiagnosticFieldVisitor {
    message: String,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl tracing::field::Visit for DiagnosticFieldVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.message = rendered;
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(rendered),
            );
        }
    }
}

/// Default dependency targets opened up when `AGENT2SSH_BRIDGE_DEPS` is enabled
/// without an explicit list. These are the transport layers whose `WARN`/`ERROR`
/// output is worth surfacing when chasing a connectivity issue.
const DEFAULT_DEP_PREFIXES: &[&str] =
    &["hyper", "reqwest", "ssh2", "h2", "rustls", "tower", "axum"];

/// A tracing layer that forwards `WARN`/`ERROR` events into the shared
/// diagnostic log (`app.log`). This unifies the daemon's rich `tracing` output
/// with the structured diagnostics the desktop UI reads, so warnings/errors are
/// observable even when the daemon's stdout/stderr are not captured.
///
/// By default only our own (`agent2ssh*`) events are bridged. Setting
/// `AGENT2SSH_BRIDGE_DEPS` opens up dependency-layer (`hyper`/`reqwest`/`ssh2`/…)
/// warnings/errors as well, for debugging transport problems (H7):
/// - `1`/`true`/`all` → the built-in [`DEFAULT_DEP_PREFIXES`] set,
/// - a comma-separated list (e.g. `ssh2,reqwest`) → exactly those target prefixes,
/// - unset/empty/`0`/`false` → off (own events only).
pub(super) struct DiagnosticBridgeLayer {
    /// Allowed dependency target prefixes. Empty = bridge only `agent2ssh*`.
    dep_prefixes: Vec<String>,
}

impl DiagnosticBridgeLayer {
    /// Build the layer from `AGENT2SSH_BRIDGE_DEPS`.
    pub(super) fn from_env() -> Self {
        let dep_prefixes = match std::env::var("AGENT2SSH_BRIDGE_DEPS") {
            Ok(raw) => Self::parse_dep_prefixes(&raw),
            Err(_) => Vec::new(),
        };
        Self { dep_prefixes }
    }

    fn parse_dep_prefixes(raw: &str) -> Vec<String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "0" | "false" | "off" | "no" => Vec::new(),
            "1" | "true" | "all" | "on" | "yes" => {
                DEFAULT_DEP_PREFIXES.iter().map(|p| p.to_string()).collect()
            }
            _ => raw
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for DiagnosticBridgeLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let level = *event.metadata().level();
        // In tracing's ordering, ERROR < WARN < INFO, so `<= WARN` keeps exactly
        // the warning and error levels. This is the primary noise guard.
        if level > tracing::Level::WARN {
            return;
        }
        let target = event.metadata().target();
        let is_own = target.starts_with("agent2ssh");
        if !is_own {
            // Dependency events only pass when explicitly opted in via a matching
            // prefix; otherwise hyper/reqwest/... noise stays in the fmt sink.
            if !self.dep_prefixes.iter().any(|p| target.starts_with(p)) {
                return;
            }
        }

        let mut visitor = DiagnosticFieldVisitor::default();
        event.record(&mut visitor);
        visitor.fields.insert(
            "target".to_string(),
            serde_json::Value::String(target.to_string()),
        );
        // Tag with the current request's correlation id when emitted inside a
        // request scope, linking daemon log lines to the originating operation.
        if let Ok(trace_id) = REQUEST_TRACE_ID.try_with(String::clone) {
            visitor
                .fields
                .insert("trace_id".to_string(), serde_json::Value::String(trace_id));
        }

        let diag_level = if level == tracing::Level::ERROR {
            "error"
        } else {
            "warn"
        };
        if is_own {
            let _ = agent2ssh::append_diagnostic_log(
                diag_level,
                "daemon",
                &visitor.message,
                Some(serde_json::Value::Object(visitor.fields)),
            );
        } else {
            // Dependency errors must NOT fan out to the error sink: the sink fires
            // a webhook over `reqwest`, and a `reqwest` failure there would bridge
            // straight back to another error → webhook → … loop. Persist for
            // observability only. (H7 anti-loop)
            let _ = agent2ssh::append_diagnostic_log_no_sink(
                diag_level,
                "deps",
                &visitor.message,
                Some(serde_json::Value::Object(visitor.fields)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dep_prefixes_parse_modes() {
        assert!(DiagnosticBridgeLayer::parse_dep_prefixes("").is_empty());
        assert!(DiagnosticBridgeLayer::parse_dep_prefixes("0").is_empty());
        assert!(DiagnosticBridgeLayer::parse_dep_prefixes("false").is_empty());

        let all = DiagnosticBridgeLayer::parse_dep_prefixes("all");
        assert_eq!(all.len(), DEFAULT_DEP_PREFIXES.len());
        assert!(all.iter().any(|p| p == "ssh2"));
        assert_eq!(DiagnosticBridgeLayer::parse_dep_prefixes("1"), all);

        let custom = DiagnosticBridgeLayer::parse_dep_prefixes(" ssh2 , reqwest ");
        assert_eq!(custom, vec!["ssh2".to_string(), "reqwest".to_string()]);
    }
}
