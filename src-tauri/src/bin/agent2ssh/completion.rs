//! Read-only completion candidates for the `agent2ssh` CLI.
//!
//! Completion runs during shell input, before normal CLI startup. Keep this
//! module side-effect free: it may read existing configuration or query GET
//! endpoints, but must never create the config directory, migrate secrets, or
//! start the daemon.

use clap_complete::engine::CompletionCandidate;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

const COMPLETION_HTTP_TIMEOUT: Duration = Duration::from_millis(200);

pub(crate) fn host_candidates() -> Vec<CompletionCandidate> {
    let Some(value) = read_json("hosts.json") else {
        return Vec::new();
    };
    values_from_array(&value, "hosts", "name")
}

pub(crate) fn playbook_candidates() -> Vec<CompletionCandidate> {
    let Some(raw) = read_config_file("playbooks.toml") else {
        return Vec::new();
    };
    let Ok(value) = raw.parse::<toml::Value>() else {
        return Vec::new();
    };
    let names = value
        .get("playbooks")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("name").and_then(toml::Value::as_str));
    candidates(names)
}

pub(crate) fn daemon_candidates() -> Vec<CompletionCandidate> {
    let mut names = vec!["localhost".to_string()];
    if let Some(raw) = read_config_file("remotes.toml") {
        if let Ok(value) = raw.parse::<toml::Value>() {
            names.extend(
                value
                    .get("remotes")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.get("alias").and_then(toml::Value::as_str))
                    .map(str::to_owned),
            );
        }
    }
    candidates(names)
}

pub(crate) fn session_candidates() -> Vec<CompletionCandidate> {
    let Some(value) = local_daemon_get("sessions") else {
        return Vec::new();
    };
    values_from_root_array(&value, "id")
}

pub(crate) fn forward_candidates() -> Vec<CompletionCandidate> {
    let Some(value) = local_daemon_get("forwards") else {
        return Vec::new();
    };
    values_from_root_array(&value, "id")
}

fn config_dir() -> Option<PathBuf> {
    agent2ssh::store::config_dir().ok()
}

fn read_config_file(name: &str) -> Option<String> {
    let path = config_dir()?.join(name);
    std::fs::read_to_string(path).ok()
}

fn read_json(name: &str) -> Option<Value> {
    serde_json::from_str(&read_config_file(name)?).ok()
}

fn values_from_array(value: &Value, array_key: &str, value_key: &str) -> Vec<CompletionCandidate> {
    let values = value
        .get(array_key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get(value_key).and_then(Value::as_str));
    candidates(values)
}

fn values_from_root_array(value: &Value, value_key: &str) -> Vec<CompletionCandidate> {
    let values = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get(value_key).and_then(Value::as_str));
    candidates(values)
}

fn candidates<I, S>(values: I) -> Vec<CompletionCandidate>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    values
        .into_iter()
        .filter_map(|value| {
            let value = value.as_ref().trim();
            (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}

fn local_daemon_get(path: &str) -> Option<Value> {
    // Requiring an existing token avoids probing the daemon on fresh installs
    // and, importantly, avoids the token-creation path used by daemon startup.
    let token = read_config_file("daemon.token")?;
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(COMPLETION_HTTP_TIMEOUT)
        .timeout(COMPLETION_HTTP_TIMEOUT)
        .build()
        .ok()?;
    client
        .get(format!(
            "{}/{}",
            agent2ssh::local_daemon_url().trim_end_matches('/'),
            path.trim_start_matches('/')
        ))
        .bearer_auth(token)
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .ok()
}
