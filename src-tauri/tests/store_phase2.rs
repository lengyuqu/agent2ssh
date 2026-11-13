//! CNB #26 phase 2 regressions: content-tagged credential references.
//!
//! Every case drives the real CLI against its own `AGENT2SSH_CONFIG_DIR` with
//! synthetic markers only; no real credential and never `~/.agent2ssh`.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use agent2ssh::secrets::{parse_secret_ref, SecretRef, SECRET_REF};

const MASTER: &str = "phase2-master-only";
const NEW_MASTER: &str = "phase2-new-master-only";
const SECRET_A: &str = "phase2-secret-a-marker";
const SECRET_B: &str = "phase2-secret-b-marker";
const HOST: &str = "phase2-host";
const ACCOUNT: &str = "host:phase2-host";

fn cli(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent2ssh"));
    command
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
        .env_remove("AGENT2SSH_MASTER_PASSWORD")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run(dir: &Path, args: &[&str], stdin: Option<&[u8]>, master: bool) -> Output {
    let mut command = cli(dir);
    command.args(args);
    if master {
        command.env("AGENT2SSH_MASTER_PASSWORD", MASTER);
    }
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(bytes) = stdin {
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn seed(dir: &Path) {
    let init = run(
        dir,
        &["secrets", "set-password"],
        Some(format!("{MASTER}\n").as_bytes()),
        false,
    );
    assert!(
        init.status.success(),
        "master init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    let add = run(
        dir,
        &[
            "host",
            "add",
            HOST,
            "--host",
            "127.0.0.1",
            "--password-stdin",
        ],
        Some(SECRET_A.as_bytes()),
        true,
    );
    assert!(
        add.status.success(),
        "seed failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(!String::from_utf8_lossy(&add.stdout).contains(SECRET_A));
    assert!(!String::from_utf8_lossy(&add.stderr).contains(SECRET_A));
}

fn hosts_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(dir.join("hosts.json")).unwrap()).unwrap()
}

fn secrets_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(dir.join("secrets.enc")).unwrap()).unwrap()
}

/// Look the host up by name: `host add` prepends new hosts, so index 0 is not
/// stable across saves.
fn host_index(hosts: &serde_json::Value) -> usize {
    hosts["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .position(|host| host["name"].as_str() == Some(HOST))
        .unwrap_or_else(|| panic!("missing {HOST}: {hosts}"))
}

fn password_ref(dir: &Path) -> String {
    let hosts = hosts_json(dir);
    let index = host_index(&hosts);
    hosts["hosts"][index]["password"]
        .as_str()
        .unwrap_or_else(|| panic!("no password on {HOST}: {hosts}"))
        .to_string()
}

fn store_tag(dir: &Path) -> Option<String> {
    secrets_json(dir)["entries"][ACCOUNT]["tag"]
        .as_str()
        .map(str::to_string)
}

fn replace_ref(dir: &Path, new_ref: &str) {
    let mut hosts = hosts_json(dir);
    let index = host_index(&hosts);
    hosts["hosts"][index]["password"] = serde_json::Value::String(new_ref.to_string());
    fs::write(
        dir.join("hosts.json"),
        serde_json::to_string_pretty(&hosts).unwrap(),
    )
    .unwrap();
}

/// Run library-level secret operations in an isolated config dir. Called only
/// from `#[serial_test::serial]` tests because it mutates process env/globals.
fn with_store<F: FnOnce()>(dir: &Path, f: F) {
    std::env::set_var("AGENT2SSH_CONFIG_DIR", dir);
    std::env::set_var("AGENT2SSH_SECRETS_BACKEND", "encrypted");
    std::env::remove_var("AGENT2SSH_MASTER_PASSWORD");
    agent2ssh::secrets::lock();
    f();
    agent2ssh::secrets::lock();
    std::env::remove_var("AGENT2SSH_CONFIG_DIR");
    std::env::remove_var("AGENT2SSH_SECRETS_BACKEND");
}

#[test]
#[serial_test::serial]
fn new_writes_carry_a_content_tag_that_survives_unrelated_saves() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());

    let reference = password_ref(dir.path());
    let tag = match parse_secret_ref(&reference) {
        Some(SecretRef::Tagged(tag)) => tag.to_string(),
        other => panic!("expected a tagged reference, got {other:?} ({reference})"),
    };
    assert!(agent2ssh::secrets::is_valid_tag(&tag), "invalid tag {tag}");
    assert_eq!(store_tag(dir.path()).as_deref(), Some(tag.as_str()));

    // Decrypt through the library: the reference must resolve to the right value.
    with_store(dir.path(), || {
        agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
        assert_eq!(
            agent2ssh::secrets::get_secret(ACCOUNT).as_deref(),
            Some(SECRET_A)
        );
    });

    // An unrelated save loads the config (resolving the tagged reference back
    // to plaintext) and re-seals it. That pre-existing read-modify-write path
    // bumps `generation`; phase 2's guarantee is that the *content tag* of an
    // unchanged secret survives the re-encryption, so the reference stays
    // valid instead of being invalidated by every unrelated save.
    let output = run(
        dir.path(),
        &["host", "add", "phase2-extra", "--host", "127.0.0.2"],
        None,
        true,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(password_ref(dir.path()), reference, "reference drifted");
    assert_eq!(store_tag(dir.path()).as_deref(), Some(tag.as_str()));
}

#[test]
#[serial_test::serial]
fn a_mismatched_tag_never_uses_the_wrong_secret_and_keeps_the_reference() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let original = password_ref(dir.path());
    let original_tag = store_tag(dir.path()).unwrap();

    // Simulate "the credential was changed/restored behind hosts.json": rewrite
    // the ciphertext for the same account without touching hosts.json.
    with_store(dir.path(), || {
        agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
        agent2ssh::secrets::store_secret(ACCOUNT, SECRET_B).unwrap();
    });
    let new_tag = store_tag(dir.path()).unwrap();
    assert_ne!(new_tag, original_tag, "a changed secret must get a new tag");

    let output = run(
        dir.path(),
        &[
            "host",
            "add",
            "phase2-after-mismatch",
            "--host",
            "127.0.0.3",
        ],
        None,
        true,
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the save itself should still work: {stderr}"
    );
    assert!(
        stderr.contains("was written against content tag"),
        "missing explicit guidance: {stderr}"
    );
    // The reference is neither replaced with SECRET_B nor blanked to None.
    assert_eq!(
        password_ref(dir.path()),
        original,
        "reference was destroyed"
    );
    let hosts = fs::read_to_string(dir.path().join("hosts.json")).unwrap();
    assert!(!hosts.contains(SECRET_B), "wrong secret leaked into config");
    assert!(
        !stderr.contains(SECRET_B),
        "wrong secret leaked into stderr"
    );
}

#[test]
#[serial_test::serial]
fn changing_the_master_password_keeps_tags_and_references_valid() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let reference = password_ref(dir.path());
    let tag = store_tag(dir.path()).unwrap();
    let generation_before = secrets_json(dir.path())["generation"].as_u64().unwrap();

    with_store(dir.path(), || {
        agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
        agent2ssh::secrets::change_master_password(NEW_MASTER).unwrap();
        agent2ssh::secrets::lock();
        agent2ssh::secrets::unlock_or_init(NEW_MASTER).unwrap();
        assert_eq!(
            agent2ssh::secrets::get_secret(ACCOUNT).as_deref(),
            Some(SECRET_A),
            "re-encryption must not change plaintext"
        );
    });

    assert_eq!(store_tag(dir.path()).as_deref(), Some(tag.as_str()));
    assert_eq!(password_ref(dir.path()), reference);
    assert_eq!(
        secrets_json(dir.path())["generation"].as_u64().unwrap(),
        generation_before,
        "a master-password change must not bump the content generation"
    );
}

#[test]
fn a_bare_reference_is_upgraded_when_the_store_entry_still_has_its_tag() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let tag = store_tag(dir.path()).unwrap();
    replace_ref(dir.path(), SECRET_REF);
    assert_eq!(password_ref(dir.path()), SECRET_REF);

    let output = run(
        dir.path(),
        &["host", "add", "phase2-upgrade", "--host", "127.0.0.4"],
        None,
        true,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let upgraded = password_ref(dir.path());
    assert_eq!(
        parse_secret_ref(&upgraded),
        Some(SecretRef::Tagged(tag.as_str())),
        "bare marker was not upgraded: {upgraded}"
    );
}

/// Put the pair into the shape a pre-phase-2 build leaves behind: a bare
/// reference and entries whose `tag` is empty (so `is_valid_tag()` rejects it).
fn strip_entry_tags(dir: &Path) {
    let mut store = secrets_json(dir);
    for entry in store["entries"].as_object_mut().unwrap().values_mut() {
        entry["tag"] = serde_json::Value::String(String::new());
    }
    fs::write(
        dir.join("secrets.enc"),
        serde_json::to_string_pretty(&store).unwrap(),
    )
    .unwrap();
}

/// Pins a **measured limitation**, not a desired invariant (2026-11-13). The
/// test above upgrades because the store entry still carries a tag. With no tag
/// anywhere — what a pre-phase-2 build actually leaves on disk — an ordinary
/// save leaves the reference bare: `entry_tags()` drops tags that fail
/// `is_valid_tag()`, so `upgraded_bare()` has nothing to offer and the value has
/// to be written again before it gets a tag. If an upgrade path for a fully
/// legacy pair is ever added, update this test rather than treating it as the
/// contract.
#[test]
fn a_fully_legacy_pair_keeps_its_bare_reference_on_the_next_save() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    replace_ref(dir.path(), SECRET_REF);
    strip_entry_tags(dir.path());
    assert_eq!(store_tag(dir.path()).as_deref(), Some(""));

    let output = run(
        dir.path(),
        &["host", "add", "phase2-legacy", "--host", "127.0.0.6"],
        None,
        true,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        password_ref(dir.path()),
        SECRET_REF,
        "a fully legacy pair must not be rewritten into a tagged reference"
    );
    assert_eq!(
        store_tag(dir.path()).as_deref(),
        Some(""),
        "no tag can be invented for an entry that has none"
    );
}

#[test]
fn malformed_reference_is_never_re_sealed_as_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let tag_before = store_tag(dir.path()).unwrap();
    let generation_before = secrets_json(dir.path())["generation"].as_u64().unwrap();
    let malformed = "$agent2ssh-secret$:not-a-valid-tag";
    replace_ref(dir.path(), malformed);

    let output = run(
        dir.path(),
        &["host", "add", "phase2-malformed", "--host", "127.0.0.5"],
        None,
        true,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(password_ref(dir.path()), malformed);
    assert_eq!(store_tag(dir.path()).as_deref(), Some(tag_before.as_str()));
    assert_eq!(
        secrets_json(dir.path())["generation"].as_u64().unwrap(),
        generation_before,
        "a malformed reference must not trigger a new seal"
    );
    assert_eq!(parse_secret_ref(malformed), Some(SecretRef::Malformed));
}
