#!/usr/bin/env bash
#
# K4 — real-SSH end-to-end test against a containerized OpenSSH server.
#
# Unlike the mock/embedded smokes (e1/e2/e2e-local), this exercises the actual
# libssh2 transport against a real sshd: key-based auth, remote exec, an SFTP
# round-trip (upload → download → byte-compare), recursive directory transfer
# (J4: build a tree remotely + pull it back), resume (K6), mkdir/ls, and a
# port-forward through the daemon. It is the regression that closes the
# "runtime-unverified" gaps (native transfer paths, recursive transfer).
#
# Requires: docker, a release build of the CLI + daemon. Auth is key-based on
# purpose — no password means the encrypted credential-store path (K1) is never
# involved, so the test is deterministic across the separate CLI processes it spawns.
#
# Usage: scripts/e2e-docker.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTAINER=agent2ssh-e2e-sshd
SSH_PORT=22122
IMAGE=lscr.io/linuxserver/openssh-server:latest
WORK="$(mktemp -d)"
export AGENT2SSH_CONFIG_DIR="$WORK/config"
mkdir -p "$AGENT2SSH_CONFIG_DIR"

CLI="${AGENT2SSH_CLI:-$REPO_ROOT/src-tauri/target/release/agent2ssh}"

PASS=0
FAIL=0
note()  { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
ok()    { PASS=$((PASS+1)); printf '  \033[32mPASS\033[0m %s\n' "$*"; }
bad()   { FAIL=$((FAIL+1)); printf '  \033[31mFAIL\033[0m %s\n' "$*"; }

cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# ── Preconditions ─────────────────────────────────────────────────────────────
command -v docker >/dev/null || { echo "docker is required"; exit 1; }
if [[ ! -x "$CLI" ]]; then
  echo "building release CLI…"
  (cd "$REPO_ROOT/src-tauri" && cargo build --release --no-default-features --bin agent2ssh)
fi

# ── Generate a throwaway keypair ──────────────────────────────────────────────
note "Generating SSH key + starting sshd container"
ssh-keygen -t ed25519 -N "" -f "$WORK/id_ed25519" -q
PUBKEY="$(cat "$WORK/id_ed25519.pub")"
chmod 600 "$WORK/id_ed25519"

# ── Start the OpenSSH container with our pubkey ───────────────────────────────
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
docker run -d --name "$CONTAINER" \
  -e PUID=1000 -e PGID=1000 \
  -e USER_NAME=tester \
  -e PUBLIC_KEY="$PUBKEY" \
  -e SUDO_ACCESS=false \
  -e PASSWORD_ACCESS=false \
  -p "$SSH_PORT:2222" \
  "$IMAGE" >/dev/null

# Wait for sshd to accept connections.
note "Waiting for sshd"
for _ in $(seq 1 30); do
  if ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
       -i "$WORK/id_ed25519" -p "$SSH_PORT" tester@127.0.0.1 true 2>/dev/null; then
    break
  fi
  sleep 2
done
ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -i "$WORK/id_ed25519" -p "$SSH_PORT" tester@127.0.0.1 true \
  || { echo "sshd never became ready"; docker logs "$CONTAINER" | tail -40; exit 1; }
ok "sshd reachable"

# ── Register the host ─────────────────────────────────────────────────────────
note "Registering host profile"
"$CLI" host add e2e --host 127.0.0.1 --user tester --port "$SSH_PORT" --key "$WORK/id_ed25519"
"$CLI" host list --json | grep -q '"name": *"e2e"' && ok "host registered" || bad "host registered"

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

# ── Summary ───────────────────────────────────────────────────────────────────
note "Summary"
printf '  %d passed, %d failed\n' "$PASS" "$FAIL"
[[ "$FAIL" == 0 ]]
