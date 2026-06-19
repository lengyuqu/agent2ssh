use agent2ssh::remote::{load_scoped_daemon_tokens, resolve_scoped_daemon_token, DaemonScope};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};

use super::{err, AppState, ErrorBody};

#[derive(Clone)]
pub(super) struct AuthContext {
    pub(super) scope: Option<DaemonScope>,
}

#[cfg(test)]
pub(super) fn check_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthContext, (StatusCode, Json<ErrorBody>)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    authenticate_token(state, token)
}

/// Authenticate a raw bearer token (admin or scoped). The HTTP middleware calls
/// this once and stores the resulting context in request extensions for
/// handlers that need scoped authorization.
fn authenticate_token(
    state: &AppState,
    token: &str,
) -> Result<AuthContext, (StatusCode, Json<ErrorBody>)> {
    if token_matches(token, &state.token) {
        return Ok(AuthContext { scope: None });
    }

    let scoped_tokens = load_scoped_daemon_tokens().map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to load scoped daemon tokens: {e}"),
        )
    })?;
    for scoped in scoped_tokens {
        let Some(expected) = resolve_scoped_daemon_token(&scoped) else {
            continue;
        };
        if token_matches(token, &expected) {
            return Ok(AuthContext {
                scope: scoped.scope.clone(),
            });
        }
    }

    tracing::warn!("failed authentication attempt");
    Err((
        StatusCode::UNAUTHORIZED,
        Json(ErrorBody {
            error: "unauthorized".into(),
        }),
    ))
}

pub(super) fn token_matches(candidate: &str, expected: &str) -> bool {
    if expected.trim().is_empty() {
        return false;
    }

    let candidate = candidate.as_bytes();
    let expected = expected.as_bytes();
    let mut diff = candidate.len() ^ expected.len();
    let max_len = candidate.len().max(expected.len());

    for i in 0..max_len {
        let candidate_byte = candidate.get(i).copied().unwrap_or(0);
        let expected_byte = expected.get(i).copied().unwrap_or(0);
        diff |= (candidate_byte ^ expected_byte) as usize;
    }

    diff == 0
}

/// Routes that intentionally require no bearer token: the redirect root, the web
/// console HTML, and the liveness/metrics probes. Matched exactly so authed
/// siblings (e.g. `/metrics/trend`) are not accidentally exposed.
pub(super) fn is_public_path(path: &str) -> bool {
    matches!(path, "/" | "/console" | "/health" | "/metrics")
}

pub(super) fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string)
}

/// Extracts a `token` query parameter, used by browser WebSocket/SSE handshakes
/// that cannot set an `Authorization` header.
pub(super) fn query_token(uri: &axum::http::Uri) -> Option<String> {
    axum::extract::Query::<std::collections::HashMap<String, String>>::try_from_uri(uri)
        .ok()
        .and_then(|query| query.0.get("token").cloned())
}

/// Central authentication gate. Every request to a non-public route must present
/// a valid admin or scoped token (via `Authorization: Bearer` or a `?token=`
/// query parameter); otherwise it is rejected with 401 before reaching any
/// handler. The resolved `AuthContext` is inserted into request extensions so
/// handlers can perform per-target scope authorization without re-reading token
/// storage.
pub(super) async fn auth_middleware(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if is_public_path(request.uri().path()) {
        return next.run(request).await;
    }
    let token = bearer_token(request.headers())
        .or_else(|| query_token(request.uri()))
        .unwrap_or_default();
    match authenticate_token(&state, &token) {
        Ok(auth) => {
            let mut request = request;
            request.extensions_mut().insert(auth);
            next.run(request).await
        }
        Err(rejection) => rejection.into_response(),
    }
}
