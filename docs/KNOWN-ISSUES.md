# Known issues and unverified ranges

English is the source of truth for this file. 中文索引见 [docs/README.md](README.md).

This file records what is still true and still open: a wrong behavior, a design limit a caller has to know, or a range that has not been measured. Resolved items move to `CHANGELOG.md` and their IDs are not reused. Investigation write-ups are not kept in the tree.

**open** — the behavior is wrong and has a direction. **limit** — the current design, which callers need to know. **unverified** — there is no evidence either way.

## 1. Behavior

| ID | Status | What is true now |
|----|--------|------------------|
| K2 | limit | The old Dropbear hang at ~30 000 bytes does not reproduce on the `russh` transport (OpenSSH 8.8–10.3 and Dropbear 2024.85 accepted an 8 MiB upload). A peer that never ACKs a write is still not bounded: `File::write` returns `Ok(n)` or `Err`, never `Ok(0)`, so `StallGuard` does not regain control, and write acks do not use `request_timeout_secs`. `sftp_transfer.rs`, `transport/russh_impl/sftp.rs`. |
| K7 | limit | A user certificate is read only from `<key>-cert.pub` beside the private key. A host certificate is checked as the bare key it vouches for: the stored fingerprint is that key's SHA256, while OpenSSH `known_hosts` records the certificate fingerprint, so importing a `-cert-v01` line can reject a legitimate connection (`SSH host fingerprint changed`). CA chain and expiry are not checked. |
| K8 | limit | `--password` and `--passphrase` still appear in process arguments; the stdin forms do not. A locked store refuses to save a plaintext secret. New references are `$agent2ssh-secret$:<tag>`. An **unlocked** pre-phase-2 binary re-seals a tagged reference as a plaintext password — one `host list` is enough, and the same migration runs on desktop unlock and on a daemon started with `AGENT2SSH_MASTER_PASSWORD`. A locked old process leaves both files untouched. This build then refuses the credential; `config-repair --restore-pair` restores both files. `secrets retag --confirm` upgrades a bare pre-phase-2 reference and does not protect against that downgrade. Downgrade still means reverting references to the bare `$agent2ssh-secret$` marker and backing up `secrets.enc` first. An append-only credential journal is not implemented. Same-uid processes can read argv and environ on macOS; on one measured Linux container kernel they can under every `hidepid` value. `ptrace_scope` was not measured. |
| K9 | limit | Built-in algorithm defaults omit SHA-1 key exchange, CBC, and HMAC-SHA1. Servers that offer only those need `agent2ssh algo set`. The error names the command. |
| K12 | limit | The transport implements no SHA-1 key exchange. `algo set` will store `diffie-hellman-group14-sha1` and the connect fails with `unsupported SSH algorithm`. This is what OpenSSH before 6.5 offered by default. There is no local fix. |
| K13 | limit | `ai_allowed` (human-only) applies only to SSH hosts in `hosts.json`. Every `local:*` target is allowed. A local command does not need this process in order to run. Pinned by `a_local_target_is_not_gated_by_the_human_only_flag`. |
| K14 | limit | Stop is not one proved process-object binding on every platform. **macOS** default stop uses the local Mach private port: discovery does not authorize, the task audit is checked, and only an observed exit counts. It does not check a code signature, does not signal a bare PID, and does not fall back to HTTP. `POST /shutdown` is 403 unless the daemon was started with `AGENT2SSH_ALLOW_UNBOUND_HTTP_SHUTDOWN=1`. `daemon stop --allow-unbound-graceful` can stop the wrong process and is reported as unverified. Desktop quit on macOS does not stop the daemon. **Windows** publishes `daemon.instance.json` (100 ns `creation_ticks`) and binds the stop handle at that precision; a daemon with no record is still compared in whole seconds. The named-pipe control endpoint stays `Unbound`: `GetNamedPipeServerProcessId` reports the creator PID after the server has exited and after the handle has been delegated. **Everywhere:** `expect_pid` on `/shutdown` returns 409 when the listener is a different PID. It narrows a race. It does not close PID reuse, a shared token, or a same-second collision on a record-less daemon. Access denied is not evidence that the process exited. A renamed binary outside the four-name whitelist is reported healthy by status-style checks and refused by stop; quit logs why. Recovery is to restore the name, or to signal the PID out of band (`kill` on Unix, `taskkill /F` on Windows) and then `daemon start`. |
| K15 | limit | Shutdown cancels waiting approvals, session reads, and one-shot local commands, and it interrupts an established `/exec` channel. A remote command without a PTY may keep running; the outcome is recorded as unknown. Connect, channel start, blocking writes, `/exec-multi`, proxied local exec, playbooks, and health snapshots can outlive the stop wait (8 s grace, one forced termination, 2 s more). |
| K16 | limit | A relative `AGENT2SSH_CONFIG_DIR` is resolved against the cwd of the process that reads it. Two working directories are two daemons. `daemon status` / `stop` will say the health endpoint has a PID and the PID file is missing; they will not guess. Use an absolute path. |
| K17 | open | Linux desktop builds still link `glib 0.18` through `atk` (GHSA-wrw7-89jp-8q8g). `atk` has no 0.20, and `gtk` 0.18 cannot take `glib` 0.20, so the advisory stays on that copy. `glib` and `gio` 0.20 are pinned beside it and nothing links that copy of `pango` / `cairo-rs` / `gdk-pixbuf`. CLI, MCP, and the daemon (`--no-default-features`) do not link GTK. macOS and Windows desktop builds do not either. Waiting on upstream `atk` / `gtk`. |

## 2. Environment

| ID | Status | What is true now |
|----|--------|------------------|
| E3 | limit | Pulling an image with Apple container is slow and has failed once with `HTTP2 StreamClosed` before a retry succeeded. |
| E6 | limit | `container_discovery` library tests follow the local service. An installed Apple `container` whose service is stopped is reported as not running. `AGENT2SSH_CONTAINER_AUTOSTART=1` changes that. |
| E8 | unverified | `containers start` and `AGENT2SSH_CONTAINER_AUTOSTART` were tested with a fake `container` script. A real service going from stopped to started was not. |

## 3. Unverified ranges

Measured: OpenSSH 8.3 / 8.8 / 9.3 / 9.9 / 10.3 and Dropbear, on Docker, partly on Apple container, and on Windows 11 (2026-11-04, matrix 190 passed / 0 failed), for ed25519, RSA-3072, ECDSA P-256/384/521 (OpenSSH and PEM), encrypted and plaintext keys, certificates, passwords, ssh-agent (macOS/Linux; Windows Pageant is §3.1), a 1 MiB SFTP round trip, and one OpenSSH 8.3 locked to old algorithms. The script is `scripts/e2e-ssh-matrix.sh`.

| Range | Why it is not a claim of support |
|-------|-----------------------------------|
| Multi-round keyboard-interactive | The code handles more than one round. No real server has exercised it. |
| FIDO / security keys (`*_sk`) | Present in the default key list. Not tested. |
| Jump host / ProxyJump across server versions and auth methods | Only an earlier standalone test, not the matrix. |
| Port forwards and PTY sessions per server version | The matrix covers connect, exec, and SFTP. |
| Real OpenSSH before 7.x | An algorithm-limited 8.3 stands in. An old handshake can differ. |
| Servers other than OpenSSH and Dropbear | Windows OpenSSH, network gear, and commercial servers were not in the matrix. |
| Negative cases for RSA and ECDSA | Unauthorized key, wrong passphrase, and wrong principal were tested for ed25519. RSA and ECDSA were success paths. |
| Full matrix on Apple container | OpenSSH 10.3 and Dropbear only. 8.x / 9.x and the old-algorithm server ran on Docker. |
| Lazy approval storage inside the real desktop app | Unit tests and daemon integration pass. It has not been walked by hand in the desktop app. |

### 3.1 Pageant coexistence

On Windows the OpenSSH agent pipe is opened first. Pageant is tried only when that open fails. A pipe that opens and holds no matching key is treated as "there is an agent"; Pageant is not consulted, and authentication falls through to default key files. Reversing the order would fail noisily on the common host that has the OpenSSH agent service and no Pageant. The 2026-10-23 Pageant acceptance (Ed25519, RSA-3072, second-identity selection, 1 MiB SFTP) covered the path where the OpenSSH pipe could not be opened.

## 4. Maintenance

- Remove an item from this file when it is fixed, and record it in `CHANGELOG.md`.
- Register a newly found open item here. Do not reuse an ID.
- Do not put a completed plan or a measurement log back into the tree to explain an ID. The code, the tests named above, and git history are the record.
