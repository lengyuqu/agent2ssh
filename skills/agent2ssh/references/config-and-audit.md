# Audit log and local configuration

Part of the agent2ssh skill. `SKILL.md` has the overview and the rules that always
apply; this file is the detail for one area.

## Audit Log

Review recent executions:

```bash
agent2ssh audit --limit 50 --json
agent2ssh audit --host prod --risk high --json
agent2ssh audit --exit-code 1 --json
```

MCP: `ssh_audit` with optional `host`, `risk_level`, `exit_code`, `since`, `until` filters.

---

## Local Configuration

Three independent rule sets live under `~/.agent2ssh/` and are reachable from
every surface — desktop Settings, `agent2ssh <group>`, and the `ssh_*` MCP tools:

| Rule set | Group | What it changes |
|----------|-------|-----------------|
| Sensitive-data redaction | `redact` | What gets scrubbed from audit records, webhook notifications, playbook results and diagnostic exports |
| Terminal highlight | `highlight` | What the terminal paints — never what the app writes |
| SSH algorithm preferences | `algo` | The algorithms offered in the next handshake |

```bash
agent2ssh redact list --json
agent2ssh redact add 'MY_SECRET_[0-9]+' '<scrubbed>' --json
agent2ssh highlight add 'ERROR' --name Errors --color '#FF6B6B' --json
agent2ssh highlight add 'a.b' --name Literal --color '#FFD060' --literal --json
agent2ssh algo set --kex curve25519-sha256 --json
```

Things to know before calling these:

- **Removal needs `--force` / `force: true`.** `redact remove` and
  `highlight remove` refuse an unconfirmed call — exit **2** on the CLI, HTTP
  **400** from the daemon and MCP — and change nothing. `... reset` restores the
  built-in set, which is the way back from a removal.
- **A rule's pattern or keyword is its identity.** Adding a duplicate fails
  rather than overwriting it — including when it duplicates a built-in.
- **Only `redact` validates the pattern.** `redact add` rejects a pattern that
  does not compile or that matches the empty string, before anything is written.
  `highlight` does not: its rule is compiled by the terminal renderer's
  JavaScript engine, so a Rust-side check would accept a different grammar.
- **`--literal` escapes at the call site**, so the text matches as plain text
  rather than as a regex. Every rule is stored as a regex, so `highlight list`
  does not distinguish the two, and renaming a literal rule needs its new
  keyword passed with `--literal` as well or it would be escaped twice.
- **`algo set` touches only the lists you name**; `algo clear` drops the
  overrides and falls back to the built-in safe defaults. A handshake already
  open keeps the algorithms it negotiated.
- **An algorithm this build cannot negotiate is accepted at save time and then
  fails every connection.** `algo set` checks the shape of a name, not whether
  the linked transport can do it, and the preferences are applied fail-closed
  immediately before each handshake. `russh` has no SHA-1 key exchange at all, so
  `--kex diffie-hellman-group14-sha1` saves fine and then every connection reports
  it as unsupported. The built-in defaults are narrowed to what the build can
  actually negotiate. `algo clear` is the way back. Ask `algo get` rather than
  assuming a list is portable.
- **Every key format the app can generate, it can also use.** The SSH transport
  is `russh`, whose parser reads the OpenSSH container, PKCS#1 and PKCS#8 alike,
  so a key written by `ssh-keygen` works without conversion — the old advice to
  run `ssh-keygen -p -m PEM` described a libssh2 limit that no longer exists.
  `keys algorithm` reports the type this build generates by default (Ed25519) and
  `keys generate` produces it; RSA is still available as a choice.
- **No OpenSSL, Perl or NASM on any platform.** The Windows build used to compile
  OpenSSL from source to escape WinCNG's MODP-only key exchange, which cost a C
  toolchain, a full Perl and an `OPENSSL_SRC_PERL` indirection. The transport now
  uses `ring`, so nothing in the default build links OpenSSL and there is no
  `libssl*.dll`/`libcrypto*.dll` to ship.
