//! What a tool call hands back to the model.
//!
//! Three things a harness needs from a tool result, none of which the raw core
//! value gives it:
//!
//! * **A size it can afford.** Core bounds a capture at 4 MiB per stream, which
//!   protects this process's memory and nothing else — a model's context is two
//!   orders of magnitude smaller, and a harness that receives more than it
//!   allows rejects the whole result. So the result carries the head and the
//!   tail of each stream up to [`DEFAULT_INLINE_OUTPUT_BYTES`], and the full
//!   capture goes to a file the agent can read or grep.
//! * **Output it can read.** An exec result used to be pretty-printed JSON with
//!   the command's output inside a string — every newline escaped. Exec tools
//!   now answer with a one-line header followed by the output as it was
//!   printed; `structuredContent` keeps the fields for programmatic clients.
//! * **A failure it can act on.** A failed tool is a result with `isError`, not
//!   a JSON-RPC error: the latter reads as "the server is broken" to most
//!   clients and often never reaches the model. Each failure names a stable
//!   code and, where there is one, the next step.

use chrono::Utc;
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Once,
    time::{Duration, SystemTime},
};

use super::McpError;

/// How much of each output stream a result carries inline when the caller
/// names no `max_output_bytes`.
///
/// 32 KiB of text is roughly 9k tokens. A command that overflows both streams
/// stays under the 25k-token result ceiling common to current harnesses, and a
/// client that forwards `content` and `structuredContent` together still fits
/// when one stream overflows, which is the usual case.
pub(super) const DEFAULT_INLINE_OUTPUT_BYTES: usize = 32 * 1024;

/// The capture bound core applies when a request names none. Kept in step with
/// `DEFAULT_MAX_OUTPUT` in `core.rs`; it only decides whether a caller's larger
/// request has to be passed down.
const CORE_DEFAULT_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

/// Lets a harness set the inline bound once instead of on every call.
const INLINE_OUTPUT_ENV: &str = "AGENT2SSH_MCP_MAX_OUTPUT_BYTES";
/// Lets a harness put spilled output where its own file tools may read.
const OUTPUT_DIR_ENV: &str = "AGENT2SSH_MCP_OUTPUT_DIR";

const SPILL_PREFIX: &str = "a2s-output-";
const SPILL_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How far a cut may move to land on a line boundary.
const LINE_SNAP_BYTES: usize = 512;
/// How many host aliases an unknown-host hint lists.
const HOST_HINT_LIMIT: usize = 30;

/// The inline bound for one call: the caller's `max_output_bytes`, else the
/// harness-wide override, else the default.
pub(super) fn inline_output_cap(args: &Value) -> usize {
    args["max_output_bytes"]
        .as_u64()
        .map(|value| value as usize)
        .or_else(|| {
            std::env::var(INLINE_OUTPUT_ENV)
                .ok()
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_INLINE_OUTPUT_BYTES)
}

/// The capture bound to pass to core for an inline bound.
///
/// Core keeps capturing at its own default so the spilled file holds more than
/// the inline excerpt; only a caller asking for more than that default needs
/// the bound raised.
pub(super) fn capture_cap(inline_cap: usize) -> Option<usize> {
    (inline_cap > CORE_DEFAULT_CAPTURE_BYTES).then_some(inline_cap)
}

fn output_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(OUTPUT_DIR_ENV).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    agent2ssh::store::config_dir()
        .ok()
        .map(|dir| dir.join("outputs"))
}

/// Remove this server's own spilled files once they are a week old.
///
/// Only names carrying [`SPILL_PREFIX`] are touched: the directory may be one a
/// harness chose, and it is not ours to tidy beyond what we put there.
fn prune_spilled_outputs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(SPILL_PREFIX) || !name.ends_with(".log") {
            continue;
        }
        let expired = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > SPILL_RETENTION);
        if expired {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Write one full stream to a file and return where it went.
///
/// `None` means the excerpt is all the caller gets — reported in the excerpt's
/// own marker rather than failing a command that did run.
fn spill(stream_name: &str, content: &str) -> Option<PathBuf> {
    static PRUNE: Once = Once::new();
    let dir = output_dir()?;
    fs::create_dir_all(&dir).ok()?;
    PRUNE.call_once(|| prune_spilled_outputs(&dir));
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = dir.join(format!(
        "{SPILL_PREFIX}{}-{}-{stream_name}.log",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        &id[..8]
    ));
    fs::write(&path, content).ok()?;
    let _ = agent2ssh::store::restrict_file_to_owner(&path);
    Some(path)
}

/// The head and tail of `stream` within `cap` bytes, with a marker for the
/// omitted middle. `None` when the stream already fits.
///
/// The tail is kept because that is where a failing command says why.
fn excerpt(stream: &str, cap: usize) -> Option<(String, usize, usize)> {
    if stream.len() <= cap {
        return None;
    }
    let mut head_end = cap / 2;
    while !stream.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = stream.len() - (cap - cap / 2);
    while !stream.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    // Prefer whole lines when one ends close to the cut.
    if let Some(newline) = stream[..head_end].rfind('\n') {
        if head_end - newline <= LINE_SNAP_BYTES {
            head_end = newline + 1;
        }
    }
    if let Some(newline) = stream[tail_start..].find('\n') {
        if newline < LINE_SNAP_BYTES {
            tail_start += newline + 1;
        }
    }
    Some((
        stream[..head_end].to_string(),
        head_end,
        tail_start.max(head_end),
    ))
}

fn clip_stream(stream: &str, stream_name: &str, cap: usize) -> Option<(String, Option<PathBuf>)> {
    let (head, head_end, tail_start) = excerpt(stream, cap)?;
    let file = spill(stream_name, stream);
    let omitted = tail_start - head_end;
    let whereabouts = match &file {
        Some(path) => format!(
            "full {stream_name} ({} bytes) saved to {}",
            stream.len(),
            path.display()
        ),
        None => format!(
            "full {stream_name} was {} bytes and could not be saved; rerun with a narrower command",
            stream.len()
        ),
    };
    let separator = if head.ends_with('\n') || head.is_empty() {
        ""
    } else {
        "\n"
    };
    Some((
        format!(
            "{head}{separator}[... {omitted} bytes omitted; {whereabouts} ...]\n{}",
            &stream[tail_start..]
        ),
        file,
    ))
}

/// Bound every `stdout` / `stderr` string in a payload, wherever it sits.
///
/// Walking the value rather than naming tools is what keeps a per-host result
/// inside `ssh_exec_multi` or a playbook step under the same bound as a single
/// exec. A clipped stream gains `<stream>_total_bytes` and, when the full text
/// was saved, `<stream>_file`.
pub(super) fn shape_outputs(value: &mut Value, cap: usize) {
    match value {
        Value::Object(map) => {
            for stream_name in ["stdout", "stderr"] {
                let Some(Value::String(stream)) = map.get(stream_name) else {
                    continue;
                };
                let total = stream.len();
                let Some((clipped, file)) = clip_stream(stream, stream_name, cap) else {
                    continue;
                };
                map.insert(stream_name.to_string(), Value::String(clipped));
                map.insert(format!("{stream_name}_total_bytes"), json!(total));
                if let Some(file) = file {
                    map.insert(
                        format!("{stream_name}_file"),
                        json!(file.display().to_string()),
                    );
                }
            }
            for child in map.values_mut() {
                shape_outputs(child, cap);
            }
        }
        Value::Array(items) => {
            for item in items {
                shape_outputs(item, cap);
            }
        }
        _ => {}
    }
}

fn push_stream_section(text: &mut String, label: &str, stream: Option<&Value>) {
    let Some(stream) = stream.and_then(Value::as_str).filter(|s| !s.is_empty()) else {
        return;
    };
    text.push_str(&format!("\n[{label}]\n"));
    text.push_str(stream);
    if !stream.ends_with('\n') {
        text.push('\n');
    }
}

/// An exec result as a header line plus the output as the command printed it.
fn render_exec_text(result: &Map<String, Value>) -> String {
    let mut header = Vec::new();
    header.push(match result.get("exit_code").and_then(Value::as_i64) {
        Some(code) => format!("exit_code: {code}"),
        None => "exit_code: none (no exit status reported)".to_string(),
    });
    if let Some(duration) = result.get("duration_ms").and_then(Value::as_u64) {
        header.push(format!("duration_ms: {duration}"));
    }
    if let Some(risk) = result.get("risk_level").and_then(Value::as_str) {
        header.push(format!("risk_level: {risk}"));
    }
    if let Some(host) = result.get("host").and_then(Value::as_str) {
        header.push(format!("host: {host}"));
    }
    let mut text = header.join(" | ");
    text.push('\n');
    if result.get("truncated").and_then(Value::as_bool) == Some(true) {
        let dropped = result
            .get("dropped_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        text.push_str(&format!(
            "note: the capture itself overflowed and dropped {dropped} bytes from the middle\n"
        ));
    }
    if result.get("risk_unclassified").and_then(Value::as_bool) == Some(true) {
        text.push_str(
            "note: risk_level is a bash verdict about a non-bash shell and may be wrong\n",
        );
    }
    let stdout = result.get("stdout");
    let stderr = result.get("stderr");
    let empty = |stream: Option<&Value>| stream.and_then(Value::as_str).is_none_or(str::is_empty);
    if empty(stdout) && empty(stderr) {
        text.push_str("(no output)\n");
    }
    push_stream_section(&mut text, "stdout", stdout);
    push_stream_section(&mut text, "stderr", stderr);
    text
}

/// `structuredContent` as the protocol defines it: a JSON object.
///
/// A tool whose natural answer is a list (`ssh_list_hosts`, `ssh_audit`,
/// `ssh_ping`, ...) or a bare value is wrapped as `{"result": ...}`. Sent as it
/// was, such a result fails the object schema a strict client validates it
/// against, and the whole answer is rejected — for `ssh_list_hosts`, the first
/// tool an agent calls. The `content` text is untouched: a model reads the list
/// itself, not the wrapper.
fn structured_object(payload: Value) -> Value {
    match payload {
        Value::Object(_) => payload,
        other => json!({ "result": other }),
    }
}

/// Wrap a tool's payload as a successful MCP result.
///
/// `exec_like` marks the tools whose payload is one command's result; those
/// are rendered as text. Everything else is compact JSON — the model reads it
/// as well as the pretty form and it costs a quarter fewer tokens.
pub(super) fn tool_success(
    exec_like: bool,
    mut payload: Value,
    cap: usize,
) -> serde_json::Result<Value> {
    shape_outputs(&mut payload, cap);
    let text = match &payload {
        Value::Object(result)
            if exec_like && result.get("stdout").is_some_and(Value::is_string) =>
        {
            render_exec_text(result)
        }
        other => serde_json::to_string(other)?,
    };
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": structured_object(payload)
    }))
}

fn known_hosts_hint() -> String {
    let mut names: Vec<String> = agent2ssh::list_hosts_core()
        .unwrap_or_default()
        .into_iter()
        .map(|host| host.name)
        .collect();
    names.sort();
    if names.is_empty() {
        return "No SSH hosts are configured. Add one with ssh_add_host or ssh_import_config. \
                To run on this machine use local_exec with host 'local'."
            .to_string();
    }
    let total = names.len();
    names.truncate(HOST_HINT_LIMIT);
    let more = if total > names.len() {
        format!(" (and {} more — call ssh_list_hosts)", total - names.len())
    } else {
        String::new()
    };
    format!(
        "Configured hosts: {}{more}. Pass one of these aliases as `host`.",
        names.join(", ")
    )
}

/// A stable code for a failure, and the next step where there is one.
///
/// A failure that was constructed with a code keeps it. The rest are still
/// `anyhow` sentences from core, recognised by the wording core uses in
/// `agent2ssh::error_codes::code_for_message` — shared with the CLI's `--json`
/// failure object so a harness sees one set of codes.
fn classify(error: &McpError) -> (&'static str, Option<String>) {
    let code = error
        .kind
        .unwrap_or_else(|| agent2ssh::error_codes::code_for_message(&error.message));

    let hint = error.hint.clone().or_else(|| match code {
        "host_not_found" => Some(known_hosts_hint()),
        "force_required" => Some(
            "This command is classified high risk. Tell the user exactly what it does, and \
             retry with force=true only after they confirm."
                .to_string(),
        ),
        "command_blocked" => Some(
            "Blocked commands cannot be run, with or without force. Do not retry; tell the \
             user what was refused."
                .to_string(),
        ),
        "approval_required" => Some(
            "An approval policy covers this command and this tool cannot raise the approval \
             prompt. Ask the user to run it from the desktop app or adjust the policy."
                .to_string(),
        ),
        "gate_paused" => Some(
            "Execution is paused by the global gate: a person pressed stop. Do not retry; \
             ask the user to resume it from the desktop app or with `agent2ssh resume`."
                .to_string(),
        ),
        "rate_limited" => Some(
            "A rate or session limit is in force. Wait a moment and retry, or run fewer \
             commands at once."
                .to_string(),
        ),
        "approval_rejected" => {
            Some("A person rejected this command. Do not retry it unchanged.".to_string())
        }
        "approval_timed_out" => Some(
            "Nobody answered the approval prompt in time. Ask the user before retrying."
                .to_string(),
        ),
        "ssh_auth_failed" => Some(
            "The host rejected the configured credentials. Check the profile's user, key_path \
             or password with the user; retrying unchanged will fail the same way."
                .to_string(),
        ),
        "ssh_connect_failed" => Some(
            "The host could not be reached. ssh_ping shows whether it is reachable at all."
                .to_string(),
        ),
        _ => None,
    });
    (code, hint)
}

/// Wrap a tool failure as an MCP result with `isError`.
///
/// The text names the code so a model can branch on it without parsing prose,
/// then the hint, then — for a timeout — whatever the command printed before
/// its deadline.
pub(super) fn tool_failure(error: &McpError, cap: usize) -> Value {
    let (code, hint) = classify(error);
    let mut details = error.details.clone().unwrap_or_default();
    let mut shaped = Value::Object(std::mem::take(&mut details));
    shape_outputs(&mut shaped, cap);
    let details = shaped.as_object().cloned().unwrap_or_default();

    let mut text = format!("Error [{code}]: {}\n", error.message);
    if let Some(hint) = &hint {
        text.push_str(&format!("Hint: {hint}\n"));
    }
    push_stream_section(
        &mut text,
        "stdout before the timeout",
        details.get("stdout"),
    );
    push_stream_section(
        &mut text,
        "stderr before the timeout",
        details.get("stderr"),
    );

    let mut structured = Map::new();
    structured.insert(
        "error".to_string(),
        json!({ "code": code, "message": error.message, "hint": hint }),
    );
    structured.extend(details);
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": true,
        "structuredContent": structured
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(message: &str) -> McpError {
        McpError::internal(message)
    }

    #[test]
    fn a_stream_within_the_bound_is_left_alone() {
        let mut payload = json!({ "stdout": "hello\n", "stderr": "" });
        shape_outputs(&mut payload, 64);
        assert_eq!(payload, json!({ "stdout": "hello\n", "stderr": "" }));
    }

    #[test]
    fn an_oversized_stream_keeps_its_head_and_tail() {
        let lines: String = (0..2000).map(|n| format!("line {n}\n")).collect();
        let (head, head_end, tail_start) = excerpt(&lines, 400).expect("must be clipped");
        assert!(head.starts_with("line 0\n"));
        assert!(head.ends_with('\n'), "the head must end on a whole line");
        assert!(lines[tail_start..].ends_with("line 1999\n"));
        assert!(
            lines[tail_start..].starts_with("line "),
            "the tail must start on a whole line"
        );
        assert!(head_end < tail_start);
        assert!(head.len() + (lines.len() - tail_start) <= 400);
    }

    #[test]
    fn a_cut_never_splits_a_multibyte_character() {
        // No newlines, so the cuts cannot snap to a line and must find a
        // character boundary on their own.
        let text = "汉".repeat(500);
        let (head, _, tail_start) = excerpt(&text, 100).expect("must be clipped");
        assert!(head.chars().all(|ch| ch == '汉'));
        assert!(text[tail_start..].chars().all(|ch| ch == '汉'));
    }

    #[test]
    fn nested_results_are_bounded_too() {
        let big = "x".repeat(5000);
        let mut payload = json!({ "results": [{ "host": "a", "result": { "stdout": big } }] });
        shape_outputs(&mut payload, 200);
        let result = &payload["results"][0]["result"];
        assert_eq!(result["stdout_total_bytes"], 5000);
        let stdout = result["stdout"].as_str().unwrap();
        assert!(stdout.contains("bytes omitted"));
        assert!(stdout.len() < 1000);
    }

    #[test]
    fn exec_text_shows_output_unescaped() {
        let result = json!({
            "host": "web1", "exit_code": 2, "duration_ms": 41, "risk_level": "low",
            "stdout": "a\nb\n", "stderr": "boom"
        });
        let text = render_exec_text(result.as_object().unwrap());
        assert_eq!(
            text,
            "exit_code: 2 | duration_ms: 41 | risk_level: low | host: web1\n\
             \n[stdout]\na\nb\n\n[stderr]\nboom\n"
        );
    }

    #[test]
    fn exec_text_says_when_there_was_no_output() {
        let result = json!({ "exit_code": 0, "stdout": "", "stderr": "" });
        let text = render_exec_text(result.as_object().unwrap());
        assert_eq!(text, "exit_code: 0\n(no output)\n");
    }

    #[test]
    fn structured_content_is_always_an_object() {
        // An object is sent as it is.
        let object = tool_success(false, json!({ "risk_level": "low" }), 1024).unwrap();
        assert_eq!(object["structuredContent"], json!({ "risk_level": "low" }));

        // A list is wrapped; the text the model reads stays the list itself.
        let list = tool_success(false, json!([{ "name": "web1" }]), 1024).unwrap();
        assert_eq!(
            list["structuredContent"],
            json!({ "result": [{ "name": "web1" }] })
        );
        assert_eq!(list["content"][0]["text"], r#"[{"name":"web1"}]"#);

        // An empty list is still an object, and so is a bare value.
        let empty = tool_success(false, json!([]), 1024).unwrap();
        assert_eq!(empty["structuredContent"], json!({ "result": [] }));
        assert_eq!(empty["content"][0]["text"], "[]");
        let null = tool_success(false, Value::Null, 1024).unwrap();
        assert_eq!(null["structuredContent"], json!({ "result": null }));
    }

    #[test]
    fn only_exec_tools_are_rendered_as_text() {
        let payload = json!({ "exit_code": 0, "stdout": "ok\n", "stderr": "" });
        let exec = tool_success(true, payload.clone(), 1024).unwrap();
        assert!(exec["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("exit_code: 0"));
        assert_eq!(exec["structuredContent"]["stdout"], "ok\n");
        let other = tool_success(false, payload, 1024).unwrap();
        assert!(other["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with('{'));
        assert!(exec.get("isError").is_none());
    }

    #[test]
    fn failures_are_classified_by_the_wording_core_uses() {
        for (message, code) in [
            (
                "command requires force=true (risk=high): 'rm -rf x'",
                "force_required",
            ),
            (
                "command blocked by risk policy: 'mkfs /dev/sda'",
                "command_blocked",
            ),
            (
                "approval required but no local MCP approval handler is available; run through the daemon approval flow",
                "approval_required",
            ),
            ("approval request timed out", "approval_timed_out"),
            ("execution gate paused", "gate_paused"),
            (
                "local daemon /exec returned HTTP 429 Too Many Requests: execution limit exceeded: host current=5 limit=5",
                "rate_limited",
            ),
            ("command rejected by approver", "approval_rejected"),
            ("SSH authentication failed for 'web1'", "ssh_auth_failed"),
            (
                "failed to connect to 10.0.0.1:22: refused",
                "ssh_connect_failed",
            ),
            ("something else entirely", "tool_error"),
        ] {
            assert_eq!(classify(&failure(message)).0, code, "{message}");
        }
    }

    #[test]
    fn an_explicit_code_wins_over_the_wording() {
        let error = McpError::invalid_args("host required");
        assert_eq!(classify(&error).0, "invalid_arguments");
    }

    #[test]
    fn a_failure_result_is_flagged_and_structured() {
        let mut details = Map::new();
        details.insert("stdout".to_string(), json!("step 1\nstep 2\n"));
        details.insert("timed_out".to_string(), json!(true));
        let error = McpError::internal("SSH command timed out after 5s: 'make'")
            .with_kind("exec_timeout")
            .with_hint("raise timeout_secs")
            .with_details(details);
        let result = tool_failure(&error, 1024);
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["error"]["code"], "exec_timeout");
        assert_eq!(result["structuredContent"]["timed_out"], true);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("Error [exec_timeout]: SSH command timed out after 5s"));
        assert!(text.contains("Hint: raise timeout_secs"));
        assert!(text.contains("[stdout before the timeout]\nstep 1\nstep 2\n"));
    }
}
