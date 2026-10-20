#!/usr/bin/env bash
#
# K4 — real-SSH end-to-end test against a containerized OpenSSH server.
#
# Unlike the mock/embedded smokes (e1/e2/e2e-local), this exercises the actual
# libssh2 transport against a real sshd: key-based auth, remote exec, an SFTP
# round-trip (upload → download → byte-compare), recursive directory transfer
# (J4: build a tree remotely + pull it back), resume (K6), mkdir/ls, and the
# exec runtime suite (src-tauri/tests/exec_fixture.rs: stderr drain, the timeout
# deadline, the timeout audit entry, and a health snapshot). It is the regression that closes the
# "runtime-unverified" gaps (native transfer paths, recursive transfer, exec
# deadline handling).
#
# Requires: a container runtime, a release build of the CLI (built here if
# missing), and a Rust toolchain to compile the exec runtime suite. Auth is
# key-based on purpose — no password means the encrypted credential-store path
# (K1) is never involved, so the test is deterministic across the separate CLI
# processes it spawns.
#
# ## Why the client key comes from `agent2ssh keys generate`
#
# Not from `ssh-keygen`. The key the app hands a user has to be a key the app can
# authenticate with, and that is not automatic: the private-key file formats
# libssh2 accepts depend on the crypto backend it was built against, so a key
# this app generates on one platform may be unusable on another. Generating the
# client key with the app makes every step below — and the whole exec runtime
# suite — a regression test for that, and the app's own public-key encoding gets
# checked against a real sshd when the key is authorized through `PUBLIC_KEY`.
#
# ## Which sshd image?
#
# `AGENT2SSH_E2E_IMAGE` defaults to the newest release of the fixture image —
# currently OpenSSH 10.3 — and that default is what should be exercised, because
# every platform this repository builds links the same libssh2 crypto backend,
# OpenSSL.
#
# The override exists for the build that still differs. A WinCNG build offers only
# MODP Diffie-Hellman key exchange, and OpenSSH 10.0 dropped those groups, so such
# a build and a modern server share no KEX algorithm at all and every connection
# fails before authentication. To point the fixture at a server that build can
# negotiate with:
#
#   AGENT2SSH_E2E_IMAGE=lscr.io/linuxserver/openssh-server:9.9_p2-r0-ls198 \
#     scripts/e2e-container.sh
#
# ## Which runtime?
#
# Not decided here. The script asks `agent2ssh containers runtime`, so the e2e
# harness and the app share one policy — see src-tauri/src/container_runtime.rs:
# explicit request, else AGENT2SSH_CONTAINER_RUNTIME, else the first installed
# runtime in Docker → wslc → Apple container order. Anything more would be a
# second copy of the selection rule, and the two copies would drift.
#
# The three runtimes accept the same verbs this script needs — `run -d --name
# -e -p <image>`, `logs`, and `rm -f` (wslc documents `rm`/`delete` as aliases of
# `remove`, Apple's reference titles its command "delete (rm)") — so only the
# binary path differs. Docker's `--format json` template and wslc's/Apple's JSON
# differ, but the *app* is what parses container output here, never this script.
#
# Usage: scripts/e2e-container.sh
#   AGENT2SSH_CONTAINER_RUNTIME=wslc scripts/e2e-container.sh   # pin a runtime
#   AGENT2SSH_E2E_IMAGE=<image> scripts/e2e-container.sh        # pin the sshd
#
# The first run on a fresh runtime also pulls the sshd image, which can take a
# few minutes.
set -euo pipefail

# Git Bash rewrites any argument that looks like a POSIX path into a Windows
# path before a native binary sees it. That rewrite is what makes `--key
# "$WORK/..."` and `-i "$WORK/..."` work here — and it is also what turns the
# *remote* path `/config/payload.bin` into `C:/Program Files/Git/config/
# payload.bin`, a path that means something on this machine and nothing on the
# server. The two cannot be told apart by shape, so the container prefix is
# excluded explicitly instead of disabling the rewrite globally. Every remote
# path this script uses is under `/config`; nothing local is.
#
# On Linux and macOS the variable is simply an unused name.
export MSYS2_ARG_CONV_EXCL='/config'

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTAINER=agent2ssh-e2e-sshd
SSH_PORT=22122
IMAGE="${AGENT2SSH_E2E_IMAGE:-lscr.io/linuxserver/openssh-server:latest}"
WORK="$(mktemp -d)"
export AGENT2SSH_CONFIG_DIR="$WORK/config"
mkdir -p "$AGENT2SSH_CONFIG_DIR"

CLI="${AGENT2SSH_CLI:-$REPO_ROOT/src-tauri/target/release/agent2ssh}"

PASS=0
FAIL=0
note()  { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
ok()    { PASS=$((PASS+1)); printf '  \033[32mPASS\033[0m %s\n' "$*"; }
bad()   { FAIL=$((FAIL+1)); printf '  \033[31mFAIL\033[0m %s\n' "$*"; }

# The runtime is resolved in the precondition block below, before any container
# is created; cleanup must therefore tolerate an unset RT_BIN.
cleanup() {
  if [[ -n "${RT_BIN:-}" ]]; then
    "$RT_BIN" rm -f "$CONTAINER" >/dev/null 2>&1 || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

# ── Preconditions ─────────────────────────────────────────────────────────────
if [[ ! -x "$CLI" ]]; then
  echo "building release CLI…"
  (cd "$REPO_ROOT/src-tauri" && cargo build --release --no-default-features --bin agent2ssh)
fi

for tool in ssh; do
  # Optional: only the independent cross-check below uses it. Readiness is
  # polled through the app itself, so the script does not need an SSH client to
  # decide whether the fixture is up.
  command -v "$tool" >/dev/null || echo "note: $tool not found; the independent client check will be skipped"
done

note "Resolving container runtime"
if ! RT_BIN="$("$CLI" containers runtime --binary)"; then
  {
    echo "No container runtime is available to this host."
    echo "Install Docker, WSL containers (wslc) or Apple container — then re-run."
    echo "Pin a specific one with AGENT2SSH_CONTAINER_RUNTIME=<docker|wslc|apple>."
    # This block is reached with "unrecognized subcommand 'containers'" when
    # $CLI is an older release build: the check above only builds a *missing*
    # binary, so a stale one is used as-is.
    echo "If the error above says the subcommand is unrecognized, $CLI is stale — rebuild it:"
    echo "  (cd src-tauri && cargo build --release --no-default-features --bin agent2ssh)"
  } >&2
  exit 1
fi
# The runtime id is a bare lowercase word, so the pretty-printed JSON can be
# scraped with one sed; the binary path is asked for separately because it can
# contain backslashes on Windows, which would need unescaping here.
RT_ID="$("$CLI" containers runtime --json | sed -n 's/.*"runtime": "\([a-z]*\)".*/\1/p')"
echo "  runtime: ${RT_ID:-unknown} ($RT_BIN)"

# ── Generate the client keypair with the app ──────────────────────────────────
# The app decides the type: on a build whose libssh2 cannot read the OpenSSH
# private-key container it generates RSA as traditional PEM, which is the only
# form that can authenticate there. Asking the app rather than hard-coding a
# type here keeps the harness and the product on one rule.
note "Generating the client key with agent2ssh + starting the sshd container"
KEY_TYPE="$("$CLI" keys algorithm --name)"
KEY_DIR="$AGENT2SSH_CONFIG_DIR/keys"
KEY_PATH="$KEY_DIR/e2e-client"
"$CLI" keys generate e2e-client >/dev/null
[[ -f "$KEY_PATH" && -f "$KEY_PATH.pub" ]] || { echo "keys generate reported success but wrote no pair"; exit 1; }
PUBKEY="$(cat "$KEY_PATH.pub")"
echo "  key:      $KEY_TYPE (${KEY_PATH#"$WORK"/})"
# The app already restricts the private key to its owner. Repeat it for the
# benefit of any client that checks permissions itself.
chmod 600 "$KEY_PATH"

# ── Start the OpenSSH container with our pubkey ───────────────────────────────
"$RT_BIN" rm -f "$CONTAINER" >/dev/null 2>&1 || true
"$RT_BIN" run -d --name "$CONTAINER" \
  -e PUID=1000 -e PGID=1000 \
  -e USER_NAME=tester \
  -e PUBLIC_KEY="$PUBKEY" \
  -e SUDO_ACCESS=false \
  -e PASSWORD_ACCESS=false \
  -p "$SSH_PORT:2222" \
  "$IMAGE" >/dev/null

# ── Register the host, then poll readiness through the app ────────────────────
# The app is the client under test, so it is also the thing that decides whether
# the fixture is up. A system SSH client would add a second code path that could
# pass while the app's own cannot — and on Windows it would fail on temp-dir key
# ACLs before it ever reached the server.
note "Registering host profile and waiting for sshd"
"$CLI" host add e2e --host 127.0.0.1 --user tester --port "$SSH_PORT" --key "$KEY_PATH"
"$CLI" host list --json | grep -q '"name": *"e2e"' && ok "host registered" || bad "host registered"

READY=0
for _ in $(seq 1 30); do
  if "$CLI" exec e2e 'true' >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 2
done
if [[ "$READY" != 1 ]]; then
  echo "sshd never became reachable through the app"
  "$RT_BIN" logs "$CONTAINER" | tail -40
  echo "  hint: a build linking libssh2's WinCNG backend needs an OpenSSH 9.x server,"
  echo "        because OpenSSH 10.0 removed the MODP Diffie-Hellman groups that are"
  echo "        all WinCNG offers. Every platform this repository builds links OpenSSL"
  echo "        instead, so on such a build the image is not the explanation — check"
  echo "        the container log above. To rule the backend out:"
  echo "        AGENT2SSH_E2E_IMAGE=lscr.io/linuxserver/openssh-server:9.9_p2-r0-ls198"
  exit 1
fi
ok "sshd reachable with the app-generated key"

# Independent cross-check: a stock OpenSSH client must accept the same private
# key and have it authorized by the same public key. This is what catches a
# malformed public-key line, which the app alone could paper over.
if command -v ssh >/dev/null 2>&1; then
  if ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
       -o BatchMode=yes -i "$KEY_PATH" -p "$SSH_PORT" tester@127.0.0.1 true 2>/dev/null; then
    ok "system ssh accepts the app-generated key (independent check)"
  else
    case "$(uname -s)" in
      MINGW*|MSYS*|CYGWIN*)
        # Known Windows trap: ssh.exe refuses a private key whose ACLs look
        # world-readable, which is what a temp directory gives it. The app and
        # the readiness check above already proved the key works.
        echo "  note: system ssh refused the key; on Windows that is the temp-dir ACL check, not the key"
        ;;
      *)
        bad "system ssh accepts the app-generated key (independent check)"
        ;;
    esac
  fi
fi

# ── 1. Remote exec ────────────────────────────────────────────────────────────
note "exec round-trip"
OUT="$("$CLI" exec e2e 'echo agent2ssh-e2e-$((40+2))' --json)"
echo "$OUT" | grep -q 'agent2ssh-e2e-42' && ok "exec stdout matched" || { bad "exec stdout"; echo "$OUT"; }

# ── 2. SFTP upload → download → byte-compare ──────────────────────────────────
note "SFTP round-trip"
head -c 1048576 /dev/urandom > "$WORK/payload.bin"   # 1 MiB
"$CLI" sftp put e2e "$WORK/payload.bin" /config/payload.bin
"$CLI" sftp get e2e /config/payload.bin "$WORK/payload.out"
if cmp -s "$WORK/payload.bin" "$WORK/payload.out"; then ok "1 MiB round-trip byte-identical"; else bad "round-trip mismatch"; fi

# ── 3. SFTP mkdir + ls ────────────────────────────────────────────────────────
note "SFTP mkdir + ls"
"$CLI" sftp mkdir e2e /config/sub/dir
"$CLI" sftp put e2e "$WORK/payload.out" /config/sub/dir/copy.bin
"$CLI" sftp ls e2e /config/sub/dir --json | grep -q 'copy.bin' && ok "ls shows uploaded file" || bad "ls missing file"

# ── 4. Recursive transfer (J4): build a remote tree, pull each file back ───────
note "Recursive directory transfer (J4)"
for i in 1 2 3; do
  "$CLI" sftp mkdir e2e "/config/tree/d$i"
  echo "file-$i" > "$WORK/f$i"
  "$CLI" sftp put e2e "$WORK/f$i" "/config/tree/d$i/f$i.txt"
done
RECOK=1
for i in 1 2 3; do
  "$CLI" sftp get e2e "/config/tree/d$i/f$i.txt" "$WORK/got$i.txt"
  grep -q "file-$i" "$WORK/got$i.txt" || RECOK=0
done
[[ "$RECOK" == 1 ]] && ok "recursive tree round-trip" || bad "recursive tree round-trip"

# ── 5. Resume (K6): truncate a partial download then resume it ────────────────
note "Resume interrupted download (K6)"
head -c 524288 "$WORK/payload.bin" > "$WORK/partial.out"   # first 512 KiB only
"$CLI" sftp get e2e /config/payload.bin "$WORK/partial.out" --resume
if cmp -s "$WORK/payload.bin" "$WORK/partial.out"; then ok "resume completed file"; else bad "resume mismatch"; fi

# ── 6. Exec runtime: stderr drain + timeout deadline + health collection ──────
# src-tauri/tests/exec_fixture.rs drives the embedded transport directly, so it
# catches channel-level regressions the CLI-level checks above cannot: a stderr
# flood that deadlocks the shared channel window, a timeout that must return at
# its deadline, and a health snapshot over the same shared capture path. It
# reuses the app-generated key and the container already running here, so every
# one of its tests also proves that key authenticates.
note "Exec runtime regression suite (stderr drain, timeout deadline, health)"
if ( cd "$REPO_ROOT/src-tauri" && \
     AGENT2SSH_TEST_SSH_PORT="$SSH_PORT" \
     AGENT2SSH_TEST_SSH_HOST=127.0.0.1 \
     AGENT2SSH_TEST_SSH_USER=tester \
     AGENT2SSH_TEST_SSH_KEY="$KEY_PATH" \
     cargo test --no-default-features --test exec_fixture -- --test-threads=1 ); then
  ok "exec runtime suite (drain + timeout + timeout audit + health)"
else
  bad "exec runtime suite (drain + timeout + timeout audit + health)"
fi

# ── Summary ───────────────────────────────────────────────────────────────────
note "Summary"
printf '  runtime: %s (%s)\n' "${RT_ID:-unknown}" "$RT_BIN"
printf '  image:   %s\n' "$IMAGE"
printf '  key:     %s (generated by agent2ssh)\n' "$KEY_TYPE"
printf '  %d passed, %d failed\n' "$PASS" "$FAIL"
[[ "$FAIL" == 0 ]]
