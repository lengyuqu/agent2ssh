# Hosts, keys and credentials

Part of the agent2ssh skill. `SKILL.md` has the overview and the rules that always
apply; this file is the detail for one area.

## Host Management

Before executing commands, confirm the host exists:

```bash
agent2ssh host list --json
```

Add a new host:

```bash
agent2ssh host add prod \
  --host 10.0.0.12 --user ubuntu --port 22 --key ~/.ssh/id_ed25519 --json
```

Import all hosts from `~/.ssh/config` at once:

```bash
agent2ssh host import-config --json
# MCP: ssh_import_config
```

Remove a host:

```bash
agent2ssh host rm prod --json
```

---

## SSH Keys

Key pairs live in `~/.agent2ssh/keys/` and are referenced by `key_path` on a host.

```bash
agent2ssh keys algorithm            # always ed25519; the answer scripts should ask for
agent2ssh keys algorithm --name     # the same, as a bare word for scripts
agent2ssh keys algorithm --json     # adds bits + reads_openssh_private_keys
agent2ssh keys generate id-work     # prints the private key path, nothing else
agent2ssh keys generate id-work --type rsa --json
agent2ssh host add web1 --host 10.0.0.5 --key "$(agent2ssh keys generate web1)"
```

Three things matter before generating:

- **Ask, do not assume.** `keys algorithm` is still the interface a script should
  read, but the answer is now constant: every build recommends `ed25519`, because
  the transport (`russh` + `ring`) reads every private-key format this app writes
  and the OS is not a factor. `keys generate` with no `--type` follows that
  recommendation on its own, which is what makes the one-liner above safe. The
  old reason — "this build might not read the OpenSSH container" — has no build
  left to be true of.
- **RSA here means traditional PEM.** 3072-bit, `-----BEGIN RSA PRIVATE KEY-----`
  (the shape `rsa` itself emits, which `russh` reads like any other format). The
  public half is still the standard single-line `ssh-rsa …` that
  `authorized_keys` expects. Sizes below 2048 are refused.
- **Replacing an existing name fails** rather than overwriting it, and the
  generated private key is restricted to its owner (0600). Deleting a key does
  not update hosts that reference it.

---

## Credentials

Host passwords and key passphrases live in `~/.agent2ssh/secrets.enc`, an
app-managed store opened with a master password (Argon2id + AES-256-GCM).
`hosts.json` holds only a `$agent2ssh-secret$` reference, so that file is safe to
sync while the store is not — and the asymmetry produces the one failure that
looks like data loss:

- **On a machine that has never unlocked the store**, every password host fails
  and appears to be "missing its password". Nothing is lost: the reference is all
  that ever synced, and `secrets.enc` is excluded from WebDAV sync on purpose —
  uploading it would put offline-guessable material for every credential on a
  third-party server.
- **Key and agent authentication are unaffected.** They never touch this store,
  so a key-based host keeps working on a fresh machine.
- **Headless surfaces need `AGENT2SSH_MASTER_PASSWORD`.** The desktop prompts;
  CLI, MCP and the daemon read the variable and cannot prompt, so without it
  password hosts are unavailable there (and the error says so). Do not put the
  master password in a command line — it would land in the process list.

To restore password hosts on a new machine, copy `secrets.enc` from the old one
by hand (same path, mode `0600`) and use the same master password. Treat that file
as a secret: it is encrypted, but losing it still means re-entering every
password.

---

## Connectivity Check

Before batch operations, verify which hosts are reachable:

```bash
agent2ssh ping web1 web2 db1 --json
# → [{host, reachable, latency_ms, error}]
```

MCP: `ssh_ping` with `hosts` array.
