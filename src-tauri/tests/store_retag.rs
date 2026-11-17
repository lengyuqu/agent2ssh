//! CNB #38 regressions: `agent2ssh secrets retag`.
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

const MASTER: &str = "retag-master-only";
const SECRET_A: &str = "retag-secret-a-marker";
const HOST: &str = "retag-host";
const ACCOUNT: &str = "host:retag-host";

fn cli(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent2ssh"));
    command
        .env("AGENT2SSH_CONFIG_DIR", dir)
        .env("AGENT2SSH_SECRETS_BACKEND", "encrypted")
        // Locked has to mean locked: the OS credential store would otherwise
        // re-unlock this scratch store behind the test's back.
        .env("AGENT2SSH_AUTO_UNLOCK", "0")
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
}

fn hosts_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(dir.join("hosts.json")).unwrap()).unwrap()
}

fn secrets_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(dir.join("secrets.enc")).unwrap()).unwrap()
}

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

/// Put the pair into the shape a pre-phase-2 build leaves behind: a bare
/// reference and entries whose `tag` is empty.
fn make_legacy(dir: &Path) {
    replace_ref(dir, SECRET_REF);
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

fn generation_pair(dir: &Path) -> (Option<u64>, Option<u64>) {
    (
        hosts_json(dir)["secrets_generation"].as_u64(),
        secrets_json(dir)["generation"].as_u64(),
    )
}

/// Assert every acceptance property of a successfully retagged pair.
fn assert_fully_tagged(dir: &Path) {
    let reference = password_ref(dir);
    let tag = match parse_secret_ref(&reference) {
        Some(SecretRef::Tagged(tag)) => tag.to_string(),
        other => panic!("expected a tagged reference, got {other:?} ({reference})"),
    };
    assert!(agent2ssh::secrets::is_valid_tag(&tag), "invalid tag {tag}");
    assert_eq!(
        store_tag(dir).as_deref(),
        Some(tag.as_str()),
        "reference tag must match the store entry"
    );
    let (hosts_generation, store_generation) = generation_pair(dir);
    assert_eq!(
        hosts_generation, store_generation,
        "generations must stay aligned"
    );
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
fn retag_upgrades_a_fully_legacy_pair_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    make_legacy(dir.path());
    assert_eq!(store_tag(dir.path()).as_deref(), Some(""));

    // Preview without --confirm: read-only. The config pair must be
    // byte-for-byte unchanged (the audit note goes to app.log, never here).
    let hosts_before = fs::read(dir.path().join("hosts.json")).unwrap();
    let secrets_before = fs::read(dir.path().join("secrets.enc")).unwrap();
    let preview = run(dir.path(), &["secrets", "retag"], None, true);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let stdout = String::from_utf8_lossy(&preview.stdout);
    assert!(
        stdout.contains("without a content tag: 1") && stdout.contains("1 (1 upgradable)"),
        "preview must name both counts: {stdout}"
    );
    assert_eq!(
        fs::read(dir.path().join("hosts.json")).unwrap(),
        hosts_before,
        "preview must not rewrite hosts.json"
    );
    assert_eq!(
        fs::read(dir.path().join("secrets.enc")).unwrap(),
        secrets_before,
        "preview must not rewrite secrets.enc"
    );

    // Apply: the bare reference becomes tagged and matches the fresh entry tag.
    let apply = run(dir.path(), &["secrets", "retag", "--confirm"], None, true);
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    assert_fully_tagged(dir.path());

    // The plaintext behind the reference is untouched.
    with_store(dir.path(), || {
        agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
        assert_eq!(
            agent2ssh::secrets::get_secret(ACCOUNT).as_deref(),
            Some(SECRET_A)
        );
    });

    // Ordinary reads keep working after the rewrite.
    let list = run(dir.path(), &["host", "list"], None, true);
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    assert!(
        String::from_utf8_lossy(&list.stdout).contains(HOST),
        "host list must still show the host"
    );

    // A diagnostic note must exist in app.log (config-repair style audit).
    let app_log = fs::read_to_string(dir.path().join("app.log")).unwrap();
    assert!(
        app_log.contains("secrets retag applied"),
        "missing audit entry: {app_log}"
    );

    // Second apply is a no-op and leaves every file byte-identical.
    let hosts_after_first = fs::read(dir.path().join("hosts.json")).unwrap();
    let secrets_after_first = fs::read(dir.path().join("secrets.enc")).unwrap();
    let again = run(dir.path(), &["secrets", "retag", "--confirm"], None, true);
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(
        String::from_utf8_lossy(&again.stdout).contains("Nothing to retag"),
        "second run must be a no-op: {}",
        String::from_utf8_lossy(&again.stdout)
    );
    assert_eq!(
        fs::read(dir.path().join("hosts.json")).unwrap(),
        hosts_after_first
    );
    assert_eq!(
        fs::read(dir.path().join("secrets.enc")).unwrap(),
        secrets_after_first
    );
    assert_fully_tagged(dir.path());
}

#[test]
#[serial_test::serial]
fn retag_preserves_unresolvable_bare_references_while_upgrading_others() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    make_legacy(dir.path());
    let mut config = hosts_json(dir.path());
    let mut orphan = config["hosts"][host_index(&config)].clone();
    orphan["name"] = serde_json::json!("orphan-host");
    orphan["host"] = serde_json::json!("127.0.0.2");
    orphan["password"] = serde_json::json!(SECRET_REF);
    config["hosts"].as_array_mut().unwrap().push(orphan);
    fs::write(
        dir.path().join("hosts.json"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    let preview = run(dir.path(), &["secrets", "retag"], None, true);
    assert!(preview.status.success());
    assert!(String::from_utf8_lossy(&preview.stdout).contains("no store entry — left as-is"));
    let result = run(dir.path(), &["secrets", "retag", "--confirm"], None, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_fully_tagged(dir.path());
    let after = hosts_json(dir.path());
    let orphan = after["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|host| host["name"] == "orphan-host")
        .unwrap();
    assert_eq!(
        orphan["password"].as_str(),
        Some(SECRET_REF),
        "unresolvable marker must remain intact"
    );
    let hosts_before = fs::read(dir.path().join("hosts.json")).unwrap();
    let secrets_before = fs::read(dir.path().join("secrets.enc")).unwrap();
    let again = run(dir.path(), &["secrets", "retag", "--confirm"], None, true);
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(String::from_utf8_lossy(&again.stdout).contains("unresolved bare reference"));
    assert_eq!(
        fs::read(dir.path().join("hosts.json")).unwrap(),
        hosts_before
    );
    assert_eq!(
        fs::read(dir.path().join("secrets.enc")).unwrap(),
        secrets_before
    );
}

#[test]
#[serial_test::serial]
fn retag_refuses_a_store_behind_hosts_before_rewriting_it() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    make_legacy(dir.path());
    let hosts_path = dir.path().join("hosts.json");
    let secrets_path = dir.path().join("secrets.enc");
    let mut hosts = hosts_json(dir.path());
    let generation = secrets_json(dir.path())["generation"].as_u64().unwrap();
    hosts["secrets_generation"] = serde_json::json!(generation + 1);
    fs::write(&hosts_path, serde_json::to_vec_pretty(&hosts).unwrap()).unwrap();
    let hosts_before = fs::read(&hosts_path).unwrap();
    let secrets_before = fs::read(&secrets_path).unwrap();
    let result = run(dir.path(), &["secrets", "retag", "--confirm"], None, true);
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("credential store is behind hosts.json")
    );
    assert_eq!(fs::read(hosts_path).unwrap(), hosts_before);
    assert_eq!(fs::read(secrets_path).unwrap(), secrets_before);
}

#[test]
#[serial_test::serial]
fn retag_does_not_rewrite_secrets_when_hosts_are_invalid_or_missing() {
    for missing in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        make_legacy(dir.path());
        let path = dir.path().join("hosts.json");
        if missing {
            fs::remove_file(&path).unwrap();
        } else {
            fs::write(&path, b"{broken").unwrap();
        }
        let secrets_before = fs::read(dir.path().join("secrets.enc")).unwrap();
        let preview = run(dir.path(), &["secrets", "retag"], None, true);
        assert!(
            !preview.status.success(),
            "invalid or missing hosts file must not look like a no-op"
        );
        let result = run(dir.path(), &["secrets", "retag", "--confirm"], None, true);
        assert!(
            !result.status.success(),
            "invalid or missing hosts file must be refused"
        );
        assert_eq!(
            fs::read(dir.path().join("secrets.enc")).unwrap(),
            secrets_before
        );
        if missing {
            assert!(!path.exists());
        } else {
            assert_eq!(fs::read(path).unwrap(), b"{broken");
        }
    }
}

#[test]
#[serial_test::serial]
fn retag_preview_refuses_a_corrupt_store_instead_of_claiming_completion() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let path = dir.path().join("secrets.enc");
    fs::write(&path, b"{broken").unwrap();
    let hosts_before = fs::read(dir.path().join("hosts.json")).unwrap();
    let preview = run(dir.path(), &["secrets", "retag"], None, false);
    assert!(!preview.status.success());
    assert!(!String::from_utf8_lossy(&preview.stdout).contains("Nothing to do"));
    assert_eq!(fs::read(path).unwrap(), b"{broken");
    assert_eq!(
        fs::read(dir.path().join("hosts.json")).unwrap(),
        hosts_before
    );
}

#[test]
#[serial_test::serial]
fn retag_writer_refuses_an_unsupported_store_version_without_rewriting() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    make_legacy(dir.path());
    let path = dir.path().join("secrets.enc");
    let mut store = secrets_json(dir.path());
    store["version"] = serde_json::json!(3);
    fs::write(&path, serde_json::to_vec_pretty(&store).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    with_store(dir.path(), || {
        agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
        let error = agent2ssh::secrets::retag_legacy_entries().unwrap_err();
        assert!(error.to_string().contains("not supported"), "{error:#}");
    });
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
#[serial_test::serial]
fn retag_refuses_a_real_v1_store_without_rewriting_it() {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use argon2::{Algorithm, Argon2, Params, Version};
    use base64::{engine::general_purpose::STANDARD, Engine};
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    replace_ref(dir.path(), SECRET_REF);
    let salt = [7u8; 16];
    let nonce = [9u8; 12];
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
        .hash_password_into(MASTER.as_bytes(), &salt, &mut key)
        .unwrap();
    let secrets = std::collections::HashMap::from([(ACCOUNT, SECRET_A)]);
    let plaintext = serde_json::to_vec(&secrets).unwrap();
    let ciphertext = Aes256Gcm::new((&key).into())
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .unwrap();
    let v1 = serde_json::json!({"version":1,"kdf":"argon2id","salt":STANDARD.encode(salt),"nonce":STANDARD.encode(nonce),"ciphertext":STANDARD.encode(ciphertext)});
    fs::write(
        dir.path().join("secrets.enc"),
        serde_json::to_vec_pretty(&v1).unwrap(),
    )
    .unwrap();
    let hosts_before = fs::read(dir.path().join("hosts.json")).unwrap();
    let secrets_before = fs::read(dir.path().join("secrets.enc")).unwrap();
    for args in [
        &["secrets", "retag"][..],
        &["secrets", "retag", "--confirm"][..],
    ] {
        let output = run(dir.path(), args, None, true);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("v1 format"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(dir.path().join("hosts.json")).unwrap(),
            hosts_before
        );
        assert_eq!(
            fs::read(dir.path().join("secrets.enc")).unwrap(),
            secrets_before
        );
    }
}

#[test]
#[serial_test::serial]
fn retag_refuses_a_locked_store_without_touching_anything() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    make_legacy(dir.path());
    let hosts_before = fs::read(dir.path().join("hosts.json")).unwrap();
    let secrets_before = fs::read(dir.path().join("secrets.enc")).unwrap();

    let refused = run(dir.path(), &["secrets", "retag", "--confirm"], None, false);
    assert!(
        !refused.status.success(),
        "a locked store must refuse --confirm"
    );
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("locked"), "refusal must say why: {stderr}");
    assert_eq!(
        fs::read(dir.path().join("hosts.json")).unwrap(),
        hosts_before,
        "hosts.json must be byte-identical after refusal"
    );
    assert_eq!(
        fs::read(dir.path().join("secrets.enc")).unwrap(),
        secrets_before,
        "secrets.enc must be byte-identical after refusal"
    );
    assert!(
        !dir.path().join(".commit.json").exists(),
        "refusal must not start a commit"
    );

    // Preview is equally harmless while locked.
    let preview = run(dir.path(), &["secrets", "retag"], None, false);
    assert!(preview.status.success());
    assert_eq!(
        fs::read(dir.path().join("hosts.json")).unwrap(),
        hosts_before
    );
    assert_eq!(
        fs::read(dir.path().join("secrets.enc")).unwrap(),
        secrets_before
    );
}

#[test]
#[serial_test::serial]
fn doctor_mentions_retag_only_while_legacy_entries_exist() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    make_legacy(dir.path());

    let before = run(dir.path(), &["doctor"], None, true);
    assert!(before.status.success());
    let stdout = String::from_utf8_lossy(&before.stdout);
    assert!(
        stdout.contains("credential tags") && stdout.contains("secrets retag"),
        "doctor must surface legacy entries while they exist: {stdout}"
    );

    let apply = run(dir.path(), &["secrets", "retag", "--confirm"], None, true);
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );

    let after = run(dir.path(), &["doctor"], None, true);
    assert!(after.status.success());
    let stdout = String::from_utf8_lossy(&after.stdout);
    assert!(
        !stdout.contains("credential tags"),
        "doctor must stay quiet once every entry is tagged: {stdout}"
    );
}

#[test]
#[serial_test::serial]
fn retag_copes_with_a_passphrase_and_a_proxy_reference() {
    const PROXY_ID: &str = "retag-proxy";
    const PROXY_SECRET: &str = "retag-proxy-secret-marker";
    const PASSPHRASE: &str = "retag-passphrase-marker";

    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let proxy_add = run(
        dir.path(),
        &["host", "add", "retag-host2", "--host", "127.0.0.2"],
        None,
        true,
    );
    assert!(
        proxy_add.status.success(),
        "{}",
        String::from_utf8_lossy(&proxy_add.stderr)
    );
    // Add a proxy with a password through the library (no dedicated CLI flag
    // in this build) and a passphrase via direct store writes, then rewrite
    // all references to the bare marker.
    with_store(dir.path(), || {
        agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
        agent2ssh::secrets::store_secret(
            &agent2ssh::secrets::proxy_account(PROXY_ID),
            PROXY_SECRET,
        )
        .unwrap();
        agent2ssh::secrets::store_secret(
            &agent2ssh::secrets::host_passphrase_account(HOST),
            PASSPHRASE,
        )
        .unwrap();
    });

    let mut hosts = hosts_json(dir.path());
    let index = host_index(&hosts);
    hosts["hosts"][index]["passphrase"] = serde_json::Value::String(SECRET_REF.to_string());
    hosts["proxies"] = serde_json::json!([{
        "id": PROXY_ID,
        "name": "retag-proxy",
        "protocol": "socks5",
        "host": "127.0.0.3",
        "port": 1080,
        "password": SECRET_REF,
    }]);
    fs::write(
        dir.path().join("hosts.json"),
        serde_json::to_string_pretty(&hosts).unwrap(),
    )
    .unwrap();
    make_legacy(dir.path());

    let apply = run(dir.path(), &["secrets", "retag", "--confirm"], None, true);
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );

    // Every bare reference must now be tagged against its own account.
    let hosts = hosts_json(dir.path());
    let index = host_index(&hosts);
    for (field, account) in [
        ("password", ACCOUNT),
        (
            "passphrase",
            agent2ssh::secrets::host_passphrase_account(HOST).as_str(),
        ),
    ] {
        let value = hosts["hosts"][index][field].as_str().unwrap();
        let tag = match parse_secret_ref(value) {
            Some(SecretRef::Tagged(tag)) => tag.to_string(),
            other => panic!("expected tagged {field}, got {other:?} ({value})"),
        };
        assert_eq!(
            secrets_json(dir.path())["entries"][account]["tag"].as_str(),
            Some(tag.as_str()),
            "{field} tag must match its entry"
        );
    }
    let proxy_ref = hosts["proxies"][0]["password"].as_str().unwrap();
    let proxy_tag = match parse_secret_ref(proxy_ref) {
        Some(SecretRef::Tagged(tag)) => tag.to_string(),
        other => panic!("expected tagged proxy password, got {other:?} ({proxy_ref})"),
    };
    assert_eq!(
        secrets_json(dir.path())["entries"][agent2ssh::secrets::proxy_account(PROXY_ID)]["tag"]
            .as_str(),
        Some(proxy_tag.as_str())
    );
    let (hosts_generation, store_generation) = generation_pair(dir.path());
    assert_eq!(hosts_generation, store_generation);

    with_store(dir.path(), || {
        agent2ssh::secrets::unlock_or_init(MASTER).unwrap();
        assert_eq!(
            agent2ssh::secrets::get_secret(ACCOUNT).as_deref(),
            Some(SECRET_A)
        );
        assert_eq!(
            agent2ssh::secrets::get_secret(&agent2ssh::secrets::proxy_account(PROXY_ID)).as_deref(),
            Some(PROXY_SECRET)
        );
        assert_eq!(
            agent2ssh::secrets::get_secret(&agent2ssh::secrets::host_passphrase_account(HOST))
                .as_deref(),
            Some(PASSPHRASE)
        );
    });
}
