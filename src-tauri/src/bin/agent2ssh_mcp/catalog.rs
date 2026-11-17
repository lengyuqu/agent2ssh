//! Which tools `tools/list` offers, and what each one says about itself.
//!
//! Every tool in a `tools/list` answer is paid for in the model's context on
//! every turn of every session, whether or not it is ever called. The registry
//! holds 76 tools — about 36 KB, roughly 10k tokens — and an agent that runs
//! commands and moves files uses a couple of dozen of them. The rest (redaction
//! and highlight rules, SSH algorithm preferences, team-config sync, webhook
//! setup, metrics) are configuration a person does in the desktop app.
//!
//! So `tools/list` offers the [`Group::Core`] tools by default and the others on
//! request (`AGENT2SSH_MCP_TOOLS`). That is a *listing* choice and nothing
//! more: every tool in the registry stays callable by name whatever the profile,
//! so a skill or prompt that names `ssh_audit_export` keeps working, and nothing
//! here is an access rule.
//!
//! The table below is the single place a tool gets its group and its hints. A
//! tool missing from it, or listed twice, fails `the_catalog_covers_every_tool`,
//! so adding a tool forces the decision instead of defaulting somewhere.

use serde_json::{json, Map, Value};

/// A set of tools that are listed together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Group {
    /// What an agent needs to run commands and move files: always listed.
    Core,
    /// Less common operations: port forwards, playbooks, snippets, recursive
    /// delete, preview and compare.
    Ops,
    /// Host, daemon, approval and team-config administration.
    Admin,
    /// Redaction and highlight rules, SSH algorithm preferences.
    Rules,
    /// Diagnostics, metrics, health and audit export.
    Diagnostics,
}

impl Group {
    const ALL: [Group; 5] = [
        Group::Core,
        Group::Ops,
        Group::Admin,
        Group::Rules,
        Group::Diagnostics,
    ];

    fn name(self) -> &'static str {
        match self {
            Group::Core => "core",
            Group::Ops => "ops",
            Group::Admin => "admin",
            Group::Rules => "rules",
            Group::Diagnostics => "diagnostics",
        }
    }

    fn parse(raw: &str) -> Option<Group> {
        Group::ALL.into_iter().find(|group| group.name() == raw)
    }

    /// One line on what is in the group, for the hint about hidden tools.
    fn summary(self) -> &'static str {
        match self {
            Group::Core => "running commands, files, sessions, connections",
            Group::Ops => "port forwards, playbooks, snippets, recursive delete, preview/compare",
            Group::Admin => "hosts, daemons, approvals, webhooks, team config sync",
            Group::Rules => "redaction and highlight rules, SSH algorithm preferences",
            Group::Diagnostics => "doctor, metrics, health snapshots, audit export",
        }
    }
}

// What a tool says about itself, as flags. Only deviations from the MCP
// defaults (not read-only, destructive, not idempotent, open-world) are ever
// sent, so a tool that does something unbounded — `ssh_exec` — carries no
// annotation at all and the list does not grow for the common case.
/// Changes nothing. The only hint sent for such a tool: the others are defined
/// only for tools that change things.
const RO: u8 = 1;
/// Changes things, but only by adding or replacing — never destroys what is
/// there beyond what the call names.
const ADD: u8 = 2;
/// Doing it twice leaves the same result as doing it once.
const IDEM: u8 = 4;
/// Touches only this machine's own state, nothing external.
const LOCAL: u8 = 8;

use Group::{Admin, Core, Diagnostics, Ops, Rules};

/// Every tool in the registry: its group and its hints.
const CATALOG: &[(&str, Group, u8)] = &[
    // ── Core ────────────────────────────────────────────────────────────────
    ("ssh_list_hosts", Core, RO | LOCAL),
    ("ssh_add_host", Core, ADD | IDEM | LOCAL),
    ("ssh_exec", Core, 0),
    ("ssh_exec_multi", Core, 0),
    ("ssh_ping", Core, RO),
    ("ssh_risk_check", Core, RO | LOCAL),
    ("ssh_audit", Core, RO | LOCAL),
    ("ssh_sftp_ls", Core, RO),
    ("ssh_sftp_stat", Core, RO),
    ("ssh_sftp_mkdir", Core, ADD | IDEM),
    ("ssh_sftp_rename", Core, 0),
    ("ssh_sftp_rm", Core, 0),
    ("ssh_sftp_upload", Core, 0),
    ("ssh_sftp_download", Core, 0),
    ("ssh_session_open", Core, ADD),
    ("ssh_session_write", Core, 0),
    ("ssh_session_read", Core, ADD),
    ("ssh_session_close", Core, ADD | IDEM),
    ("ssh_session_list", Core, RO | LOCAL),
    ("ssh_connect", Core, ADD | IDEM),
    ("ssh_disconnect", Core, ADD | IDEM),
    ("ssh_connection_status", Core, RO | LOCAL),
    ("local_exec", Core, 0),
    ("resident_session_open", Core, ADD),
    ("resident_session_exec", Core, 0),
    ("resident_session_close", Core, ADD | IDEM),
    // ── Ops ─────────────────────────────────────────────────────────────────
    ("ssh_exec_compare", Ops, 0),
    ("ssh_preview_exec", Ops, RO | LOCAL),
    ("ssh_sftp_rmdir", Ops, 0),
    ("ssh_sftp_rm_rf", Ops, 0),
    ("ssh_forward_add", Ops, ADD),
    ("ssh_forward_list", Ops, RO | LOCAL),
    ("ssh_forward_remove", Ops, ADD | IDEM),
    ("ssh_playbook_list", Ops, RO | LOCAL),
    ("ssh_playbook_run", Ops, 0),
    ("ssh_playbook_dry_run", Ops, RO | LOCAL),
    ("ssh_snippet_list", Ops, RO | LOCAL),
    ("ssh_snippet_save", Ops, ADD | IDEM | LOCAL),
    ("ssh_snippet_delete", Ops, IDEM | LOCAL),
    ("containers_start_service", Ops, ADD | IDEM | LOCAL),
    // ── Admin ───────────────────────────────────────────────────────────────
    ("ssh_list_daemons", Admin, RO | LOCAL),
    ("ssh_import_config", Admin, ADD | LOCAL),
    ("ssh_remove_host", Admin, IDEM | LOCAL),
    ("ssh_gate_status", Admin, RO | LOCAL),
    ("ssh_approval_list", Admin, RO | LOCAL),
    ("ssh_approval_respond", Admin, LOCAL),
    ("ssh_approval_policies_list", Admin, RO | LOCAL),
    ("ssh_approval_check", Admin, RO | LOCAL),
    ("ssh_webhook_config", Admin, LOCAL),
    ("ssh_config_export", Admin, RO | LOCAL),
    ("ssh_config_import", Admin, LOCAL),
    ("ssh_config_import_preview", Admin, RO | LOCAL),
    ("ssh_sync_diff", Admin, RO | LOCAL),
    ("ssh_sync_export", Admin, LOCAL),
    // ── Rules ───────────────────────────────────────────────────────────────
    ("ssh_redact_rule_list", Rules, RO | LOCAL),
    ("ssh_redact_rule_add", Rules, ADD | LOCAL),
    ("ssh_redact_rule_update", Rules, IDEM | LOCAL),
    ("ssh_redact_rule_remove", Rules, IDEM | LOCAL),
    ("ssh_redact_rule_reset", Rules, IDEM | LOCAL),
    ("ssh_highlight_rule_list", Rules, RO | LOCAL),
    ("ssh_highlight_rule_add", Rules, ADD | LOCAL),
    ("ssh_highlight_rule_update", Rules, IDEM | LOCAL),
    ("ssh_highlight_rule_remove", Rules, IDEM | LOCAL),
    ("ssh_highlight_rule_reset", Rules, IDEM | LOCAL),
    ("ssh_algo_prefs_get", Rules, RO | LOCAL),
    ("ssh_algo_prefs_set", Rules, IDEM | LOCAL),
    ("ssh_algo_prefs_clear", Rules, IDEM | LOCAL),
    // ── Diagnostics ─────────────────────────────────────────────────────────
    ("ssh_audit_export", Diagnostics, RO | LOCAL),
    ("ssh_doctor", Diagnostics, RO | LOCAL),
    ("ssh_metrics", Diagnostics, RO | LOCAL),
    ("ssh_metrics_trend", Diagnostics, RO | LOCAL),
    ("ssh_health_snapshot", Diagnostics, RO),
    ("ssh_daemon_diagnose", Diagnostics, RO),
    ("ssh_daemon_version_check", Diagnostics, RO),
    ("ssh_daemons_view", Diagnostics, RO),
    ("ssh_events_subscribe", Diagnostics, RO | LOCAL),
];

/// Environment variable naming the groups `tools/list` offers.
const PROFILE_ENV: &str = "AGENT2SSH_MCP_TOOLS";

/// Which groups `tools/list` offers.
#[derive(Debug)]
pub(super) struct Profile {
    groups: Vec<Group>,
    /// Words in the setting that name no group, kept so the model can be told
    /// instead of the setting silently doing nothing.
    ignored: Vec<String>,
}

impl Profile {
    /// `None` or empty is the default, `core`. `all` is every group. Anything
    /// else is a comma-separated list of groups to add to `core`.
    pub(super) fn parse(raw: Option<&str>) -> Self {
        let raw = raw.map(str::trim).unwrap_or_default();
        let mut groups = vec![Group::Core];
        let mut ignored = Vec::new();
        for word in raw
            .split(',')
            .map(|word| word.trim().to_ascii_lowercase())
            .filter(|word| !word.is_empty())
        {
            if word == "all" || word == "*" {
                return Self {
                    groups: Group::ALL.to_vec(),
                    ignored,
                };
            }
            match Group::parse(&word) {
                Some(group) if !groups.contains(&group) => groups.push(group),
                Some(_) => {}
                None => ignored.push(word),
            }
        }
        Self { groups, ignored }
    }

    pub(super) fn from_env() -> Self {
        Self::parse(std::env::var(PROFILE_ENV).ok().as_deref())
    }

    fn lists(&self, group: Group) -> bool {
        self.groups.contains(&group)
    }

    fn hidden_groups(&self) -> Vec<Group> {
        Group::ALL
            .into_iter()
            .filter(|group| !self.lists(*group))
            .collect()
    }
}

fn entry(name: &str) -> Option<(Group, u8)> {
    CATALOG
        .iter()
        .find(|(known, _, _)| *known == name)
        .map(|(_, group, flags)| (*group, *flags))
}

/// The `annotations` object for a tool's flags, or `None` when every hint would
/// be the MCP default.
fn annotations(flags: u8) -> Option<Value> {
    let mut hints = Map::new();
    if flags & RO != 0 {
        hints.insert("readOnlyHint".into(), json!(true));
    } else {
        if flags & ADD != 0 {
            hints.insert("destructiveHint".into(), json!(false));
        }
        if flags & IDEM != 0 {
            hints.insert("idempotentHint".into(), json!(true));
        }
    }
    if flags & LOCAL != 0 {
        hints.insert("openWorldHint".into(), json!(false));
    }
    (!hints.is_empty()).then_some(Value::Object(hints))
}

/// The registry narrowed to `profile`, each tool carrying its annotations.
///
/// A tool the catalog does not know is listed rather than dropped: a gap in the
/// table must never make a tool vanish. `the_catalog_covers_every_tool` is what
/// keeps the table complete.
pub(super) fn listed_tools(definitions: Vec<Value>, profile: &Profile) -> Vec<Value> {
    definitions
        .into_iter()
        .filter_map(|mut definition| {
            let name = definition.get("name").and_then(Value::as_str)?.to_string();
            let Some((group, flags)) = entry(&name) else {
                return Some(definition);
            };
            if !profile.lists(group) {
                return None;
            }
            if let (Some(hints), Some(object)) = (annotations(flags), definition.as_object_mut()) {
                object.insert("annotations".into(), hints);
            }
            Some(definition)
        })
        .collect()
}

/// What the server tells the model about itself at `initialize`.
///
/// Short on purpose — it is read on every session. It says what the tool
/// descriptions cannot: how results and failures are shaped, the two habits that
/// matter (confirm before `force`, `ssh_connect` before a series), and that some
/// tools are not listed.
pub(super) fn instructions(profile: &Profile, hidden_tools: usize) -> String {
    let mut text = String::from(
        "Agent2SSH runs commands and moves files on the user's SSH hosts, and with local_exec on \
         this machine. Hosts are addressed by alias; start with ssh_list_hosts.\n\
         - ssh_exec answers with a header line (exit_code, duration, risk) and then the output as \
         printed. A non-zero exit code is a normal result. Output past max_output_bytes keeps its \
         head and tail; the full text is saved to a local file named in the result.\n\
         - A failed tool returns `Error [code]: ...` with a Hint — follow it. Do not retry \
         gate_paused, command_blocked, approval_rejected or policy_denied.\n\
         - Confirm with the user before passing force=true for a high-risk command. When the \
         Agent2SSH daemon is running, a command covered by an approval policy waits for the user \
         to approve it in the app.\n\
         - For several commands on one host, call ssh_connect first: later ssh_exec calls reuse \
         the connection and are several times faster.",
    );
    if hidden_tools > 0 {
        let groups = profile
            .hidden_groups()
            .into_iter()
            .map(|group| format!("{} ({})", group.name(), group.summary()))
            .collect::<Vec<_>>()
            .join("; ");
        text.push_str(&format!(
            "\n- {hidden_tools} more tools are not listed: {groups}. They still work if called by \
             name; the user lists them by setting {PROFILE_ENV}=all (or a comma-separated list of \
             those group names) for this MCP server."
        ));
    }
    if !profile.ignored.is_empty() {
        text.push_str(&format!(
            "\n- {PROFILE_ENV} named unknown groups ({}); valid groups are {}, or all.",
            profile.ignored.join(", "),
            Group::ALL.map(Group::name).join(", ")
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn registry_names() -> Vec<String> {
        super::super::agent2ssh_mcp_tools::tool_definitions()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect()
    }

    /// The table and the registry have to be the same set. A new tool with no
    /// row here would otherwise be listed with no hints and no group, and a
    /// removed tool would leave a row describing nothing.
    #[test]
    fn the_catalog_covers_every_tool() {
        let registry: BTreeSet<String> = registry_names().into_iter().collect();
        let mut seen = BTreeSet::new();
        for (name, _, _) in CATALOG {
            assert!(
                seen.insert(name.to_string()),
                "{name} is in the catalog twice"
            );
        }
        let missing: Vec<_> = registry.difference(&seen).collect();
        let stale: Vec<_> = seen.difference(&registry).collect();
        assert!(missing.is_empty(), "tools with no catalog row: {missing:?}");
        assert!(stale.is_empty(), "catalog rows for no tool: {stale:?}");
    }

    #[test]
    fn a_read_only_tool_is_never_also_marked_as_changing_things() {
        for (name, _, flags) in CATALOG {
            if flags & RO != 0 {
                assert_eq!(flags & (ADD | IDEM), 0, "{name} is read-only and writes");
            }
        }
    }

    #[test]
    fn the_default_profile_lists_core_and_only_core() {
        let listed = listed_tools(
            super::super::agent2ssh_mcp_tools::tool_definitions(),
            &Profile::parse(None),
        );
        let names: BTreeSet<&str> = listed
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        for (name, group, _) in CATALOG {
            assert_eq!(
                names.contains(name),
                *group == Group::Core,
                "{name} ({})",
                group.name()
            );
        }
        // The tools an agent opens a session with must be in the default list.
        for must in [
            "ssh_list_hosts",
            "ssh_exec",
            "ssh_sftp_upload",
            "ssh_connect",
            "local_exec",
        ] {
            assert!(names.contains(must), "{must} is not in the default list");
        }
    }

    #[test]
    fn the_profile_setting_adds_groups_and_all_lists_everything() {
        let count = |setting: Option<&str>| {
            listed_tools(
                super::super::agent2ssh_mcp_tools::tool_definitions(),
                &Profile::parse(setting),
            )
            .len()
        };
        let core = count(None);
        assert_eq!(count(Some("")), core);
        assert_eq!(count(Some("core")), core);
        assert!(count(Some("rules")) > core);
        assert_eq!(
            count(Some(" Rules , diagnostics ")),
            core + CATALOG
                .iter()
                .filter(|(_, group, _)| matches!(group, Group::Rules | Group::Diagnostics))
                .count(),
            "words are trimmed and case-insensitive"
        );
        assert_eq!(count(Some("all")), CATALOG.len());
        assert_eq!(count(Some("*")), CATALOG.len());
        assert_eq!(count(Some("rules,all")), CATALOG.len());
    }

    #[test]
    fn unknown_groups_are_ignored_but_reported_to_the_model() {
        let profile = Profile::parse(Some("rules, bogus"));
        assert!(profile.lists(Group::Rules));
        let text = instructions(&profile, 10);
        assert!(text.contains("unknown groups (bogus)"), "{text}");
    }

    #[test]
    fn annotations_carry_only_what_differs_from_the_defaults() {
        assert_eq!(annotations(0), None, "ssh_exec says nothing");
        assert_eq!(
            annotations(RO | LOCAL),
            Some(json!({ "readOnlyHint": true, "openWorldHint": false }))
        );
        assert_eq!(
            annotations(ADD | IDEM),
            Some(json!({ "destructiveHint": false, "idempotentHint": true }))
        );
        assert_eq!(annotations(RO), Some(json!({ "readOnlyHint": true })));
        // A read-only tool never carries the write hints, whatever its flags.
        assert_eq!(
            annotations(RO | ADD | IDEM),
            Some(json!({ "readOnlyHint": true }))
        );
    }

    #[test]
    fn the_instructions_name_hidden_groups_only_when_something_is_hidden() {
        let hidden = instructions(&Profile::parse(None), 50);
        assert!(hidden.contains("50 more tools are not listed"), "{hidden}");
        assert!(
            hidden.contains("rules (redaction and highlight rules"),
            "{hidden}"
        );
        assert!(
            !hidden.contains("core ("),
            "the listed group is not 'hidden': {hidden}"
        );
        let everything = instructions(&Profile::parse(Some("all")), 0);
        assert!(!everything.contains("not listed"), "{everything}");
    }
}
