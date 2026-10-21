#!/usr/bin/env bash
#
# SSH connection-mode matrix against real servers in containers.
#
# scripts/e2e-container.sh proves one path end to end (key auth, exec, SFTP) on
# one sshd. This script widens the *connection* side: every authentication mode
# the app offers, against several versions of the mainstream server, with the
# negative cases that make a pass mean something.
#
#   mode                         expected
#   ---------------------------  --------
#   password                     accepted
#   password (wrong)             rejected
#   public key                   accepted
#   public key (not authorized)  rejected
#   encrypted key + passphrase   accepted
#   encrypted key, wrong phrase  rejected
#   user certificate             accepted   (OpenSSH only: Dropbear has none)
#   certificate, wrong principal rejected
#   certificate, expired         rejected
#
# Servers (AGENT2SSH_MATRIX_SERVERS, space separated):
#   openssh:<version>   linuxserver/openssh-server pinned to that OpenSSH
#                       (8.3 8.8 9.3 9.9 10.3 by default)
#   dropbear            Alpine's Dropbear, the embedded-device server
#
# The certificate cases need a host `ssh-keygen` to act as the CA; they are
# skipped, loudly, when it is missing. Everything else only needs the app and a
# container runtime (Docker, wslc or Apple container, chosen like
# e2e-container.sh does through `agent2ssh containers runtime`).
#
# State lives in a throwaway AGENT2SSH_CONFIG_DIR; the real ~/.agent2ssh is never
# touched.
#
# Usage: scripts/e2e-ssh-matrix.sh
#   AGENT2SSH_MATRIX_SERVERS="openssh:9.9 dropbear" scripts/e2e-ssh-matrix.sh
#   AGENT2SSH_CONTAINER_RUNTIME=apple scripts/e2e-ssh-matrix.sh
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLI="${AGENT2SSH_CLI:-$REPO_ROOT/src-tauri/target/release/agent2ssh}"
SERVERS="${AGENT2SSH_MATRIX_SERVERS:-openssh:8.3 openssh:8.8 openssh:9.3 openssh:9.9 openssh:10.3 dropbear}"
BASE_PORT="${AGENT2SSH_MATRIX_PORT:-22130}"
USER_NAME=tester
USER_PASSWORD=s3cret-pw
KEY_PASSPHRASE=key-pass-phrase

WORK="$(mktemp -d)"
export AGENT2SSH_CONFIG_DIR="$WORK/config"
# Passwords are stored in the app's encrypted store; a fixed master password keeps
# it unlockable across the separate CLI processes this script spawns.
export AGENT2SSH_MASTER_PASSWORD="matrix-master-pw"
mkdir -p "$AGENT2SSH_CONFIG_DIR"

PASS=0
FAIL=0
SKIP=0
RESULTS=()
note() { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
ok()   { PASS=$((PASS+1)); RESULTS+=("PASS  $CURRENT  $*"); printf '  \033[32mPASS\033[0m %s\n' "$*"; }
bad()  { FAIL=$((FAIL+1)); RESULTS+=("FAIL  $CURRENT  $*"); printf '  \033[31mFAIL\033[0m %s\n' "$*"; }
skip() { SKIP=$((SKIP+1)); RESULTS+=("skip  $CURRENT  $*"); printf '  \033[33mSKIP\033[0m %s\n' "$*"; }

CONTAINER=""
cleanup() {
  [[ -n "${RT_BIN:-}" && -n "$CONTAINER" ]] && "$RT_BIN" rm -f "$CONTAINER" >/dev/null 2>&1
  rm -rf "$WORK"
}
trap cleanup EXIT

if [[ ! -x "$CLI" ]]; then
  echo "building release CLI…"
  (cd "$REPO_ROOT/src-tauri" && cargo build --release --no-default-features --bin agent2ssh) || exit 1
fi

note "Resolving container runtime"
RT_BIN="$("$CLI" containers runtime --binary)" || {
  echo "No container runtime available (Docker, wslc or Apple container)." >&2
  exit 1
}
RT_ID="$("$CLI" containers runtime --json | sed -n 's/.*"runtime": "\([a-z]*\)".*/\1/p')"
echo "  runtime: ${RT_ID:-unknown} ($RT_BIN)"

# Apple container refuses lscr.io's cross-registry token exchange; the same
# images are served from ghcr.io directly.
if [[ "$RT_ID" == apple ]]; then
  OPENSSH_REPO="ghcr.io/linuxserver/openssh-server"
else
  OPENSSH_REPO="lscr.io/linuxserver/openssh-server"
fi

# Newest published build of each OpenSSH release in the linuxserver image.
openssh_tag() {
  case "$1" in
    8.3) echo version-8.3_p1-r1 ;;
    8.8) echo version-8.8_p1-r1 ;;
    9.3) echo version-9.3_p2-r1 ;;
    9.9) echo version-9.9_p2-r0 ;;
    10.3) echo version-10.3_p1-r1 ;;
    *) echo "version-$1" ;;
  esac
}

# ── Key material (client side) ───────────────────────────────────────────────
note "Generating client keys and a certificate authority"
"$CLI" keys generate plain >/dev/null || { echo "keys generate failed"; exit 1; }
KEY_PLAIN="$AGENT2SSH_CONFIG_DIR/keys/plain"
"$CLI" keys generate stranger >/dev/null
KEY_STRANGER="$AGENT2SSH_CONFIG_DIR/keys/stranger"

HAVE_KEYGEN=0
command -v ssh-keygen >/dev/null 2>&1 && HAVE_KEYGEN=1
if [[ "$HAVE_KEYGEN" == 1 ]]; then
  KEY_ENC="$WORK/enc"
  ssh-keygen -q -t ed25519 -N "$KEY_PASSPHRASE" -C matrix-enc -f "$KEY_ENC"
  ssh-keygen -q -t ed25519 -N '' -C matrix-ca -f "$WORK/ca"
  for name in cert_ok cert_principal cert_expired; do
    ssh-keygen -q -t ed25519 -N '' -C "matrix-$name" -f "$WORK/$name"
  done
  # Valid for tester; the same key signed for someone else; and one that lapsed.
  ssh-keygen -q -s "$WORK/ca" -I matrix-ok -n "$USER_NAME" -V +1h "$WORK/cert_ok.pub"
  ssh-keygen -q -s "$WORK/ca" -I matrix-principal -n somebody-else -V +1h "$WORK/cert_principal.pub"
  ssh-keygen -q -s "$WORK/ca" -I matrix-expired -n "$USER_NAME" -V -2h:-1h "$WORK/cert_expired.pub"
  echo "  keys: app-generated ed25519 x2, encrypted ed25519, CA + 3 certificates"
else
  echo "  note: no ssh-keygen on this host; encrypted-key and certificate cases will be skipped"
fi

# ── Helpers ──────────────────────────────────────────────────────────────────
# Does `agent2ssh exec <host>` run a command? 0 = connected and ran it.
connects() {
  local out
  out="$("$CLI" exec "$1" 'echo matrix-ok' --json 2>&1)" || return 1
  [[ "$out" == *matrix-ok* ]]
}

expect_accept() { # <host-profile> <description>
  if connects "$1"; then ok "$2"; else bad "$2"; "$CLI" exec "$1" true 2>&1 | tail -1 | cut -c1-200 | sed 's/^/        /'; fi
}
expect_reject() { # <host-profile> <description>
  if connects "$1"; then bad "$2 (was accepted)"; else ok "$2"; fi
}

add_host() { # <name> <port> [extra host-add args…]
  local name="$1" port="$2"
  shift 2
  "$CLI" host add "$name" --host 127.0.0.1 --port "$port" --user "$USER_NAME" "$@" >/dev/null
}

rt_exec() { "$RT_BIN" exec "$CONTAINER" "$@"; }

start_openssh() { # <version> <port>
  local tag; tag="$(openssh_tag "$1")"
  "$RT_BIN" run -d --name "$CONTAINER" \
    -e PUID=1000 -e PGID=1000 -e USER_NAME="$USER_NAME" \
    -e PUBLIC_KEY="$(cat "$KEY_PLAIN.pub")" \
    -e PASSWORD_ACCESS=true -e USER_PASSWORD="$USER_PASSWORD" -e SUDO_ACCESS=false \
    -p "$2:2222" "$OPENSSH_REPO:$tag" >/dev/null
}

start_dropbear() { # <port>
  # Alpine's Dropbear. -R generates the host keys on first use, -E logs to stderr.
  "$RT_BIN" run -d --name "$CONTAINER" \
    -e PK="$(cat "$KEY_PLAIN.pub")" -e ENCPK="$([[ "$HAVE_KEYGEN" == 1 ]] && cat "$KEY_ENC.pub")" \
    -e UN="$USER_NAME" -e UP="$USER_PASSWORD" \
    -p "$1:2222" alpine:3.20 sh -c '
      apk add --no-cache dropbear >/dev/null &&
      adduser -D -s /bin/sh "$UN" && echo "$UN:$UP" | chpasswd &&
      install -d -m 700 -o "$UN" -g "$UN" /home/$UN/.ssh &&
      printf "%s\n%s\n" "$PK" "$ENCPK" > /home/$UN/.ssh/authorized_keys &&
      chown "$UN:$UN" /home/$UN/.ssh/authorized_keys && chmod 600 /home/$UN/.ssh/authorized_keys &&
      exec dropbear -F -E -R -p 2222' >/dev/null
}

wait_ready() { # <host-profile>
  for _ in $(seq 1 60); do
    connects "$1" && return 0
    sleep 2
  done
  return 1
}

# ── The matrix ───────────────────────────────────────────────────────────────
idx=0
for server in $SERVERS; do
  CURRENT="$server"
  port=$((BASE_PORT + idx)); idx=$((idx + 1))
  kind="${server%%:*}"
  CONTAINER="agent2ssh-matrix-$idx"
  note "$server on port $port"
  "$RT_BIN" rm -f "$CONTAINER" >/dev/null 2>&1

  case "$kind" in
    openssh)
      if ! start_openssh "${server#*:}" "$port"; then bad "start $server (image pull or run failed)"; continue; fi ;;
    dropbear)
      if ! start_dropbear "$port"; then bad "start $server (image pull or run failed)"; continue; fi ;;
    *) echo "unknown server kind: $kind"; continue ;;
  esac

  # Baseline profile doubles as the readiness probe.
  add_host "$idx-key" "$port" --key "$KEY_PLAIN"
  if ! wait_ready "$idx-key"; then
    bad "server became reachable through the app"
    "$RT_BIN" logs "$CONTAINER" 2>&1 | tail -15 | sed 's/^/        /'
    continue
  fi
  ok "server became reachable through the app"

  # Server-side setup that the image's environment variables cannot express.
  # Newer images read /config/sshd/sshd_config; older ones (through 9.3) start
  # sshd with flags and read /etc/ssh/sshd_config, so look for the live file.
  SETUP_OK=1
  if [[ "$HAVE_KEYGEN" == 1 && "$kind" == openssh ]]; then
    rt_exec sh -c "echo '$(cat "$KEY_ENC.pub")' >> /config/.ssh/authorized_keys" || SETUP_OK=0
    rt_exec sh -c "
      cfg=/config/sshd/sshd_config; [ -f \$cfg ] || cfg=/etc/ssh/sshd_config
      echo '$(cat "$WORK/ca.pub")' > /config/ca.pub && chmod 644 /config/ca.pub &&
      echo 'TrustedUserCAKeys /config/ca.pub' >> \$cfg &&
      kill -HUP \$(cat /config/sshd.pid)" || SETUP_OK=0
    sleep 2
    [[ "$SETUP_OK" == 1 ]] && ok "server trusts the test CA" || bad "server-side CA setup"
  fi

  # Password
  add_host "$idx-pw" "$port" --password "$USER_PASSWORD"
  expect_accept "$idx-pw" "password"
  add_host "$idx-pw-bad" "$port" --password "not-$USER_PASSWORD"
  expect_reject "$idx-pw-bad" "wrong password is rejected"

  # Public key
  expect_accept "$idx-key" "public key"
  add_host "$idx-stranger" "$port" --key "$KEY_STRANGER"
  expect_reject "$idx-stranger" "key that is not authorized is rejected"

  # Encrypted key
  if [[ "$HAVE_KEYGEN" == 1 ]]; then
    add_host "$idx-enc" "$port" --key "$KEY_ENC" --passphrase "$KEY_PASSPHRASE"
    expect_accept "$idx-enc" "encrypted key with its passphrase"
    add_host "$idx-enc-bad" "$port" --key "$KEY_ENC" --passphrase "wrong-$KEY_PASSPHRASE"
    expect_reject "$idx-enc-bad" "encrypted key with the wrong passphrase is rejected"
  else
    skip "encrypted key (no ssh-keygen)"
  fi

  # Certificates
  if [[ "$kind" != openssh ]]; then
    skip "user certificate ($kind does not implement OpenSSH certificates)"
  elif [[ "$HAVE_KEYGEN" != 1 ]]; then
    skip "user certificate (no ssh-keygen)"
  else
    add_host "$idx-cert" "$port" --key "$WORK/cert_ok"
    expect_accept "$idx-cert" "user certificate signed by the trusted CA"
    add_host "$idx-cert-principal" "$port" --key "$WORK/cert_principal"
    expect_reject "$idx-cert-principal" "certificate for another principal is rejected"
    add_host "$idx-cert-expired" "$port" --key "$WORK/cert_expired"
    expect_reject "$idx-cert-expired" "expired certificate is rejected"
    # The bare key behind a certificate is not authorized by itself: a pass above
    # therefore proves the certificate was presented, not just the key.
  fi

  "$RT_BIN" rm -f "$CONTAINER" >/dev/null 2>&1
  CONTAINER=""
done

# ── Summary ──────────────────────────────────────────────────────────────────
note "Summary"
printf '  runtime: %s (%s)\n' "${RT_ID:-unknown}" "$RT_BIN"
printf '%s\n' "${RESULTS[@]}" | awk '
  { n[$2]++; c[$2, $1]++; if (!($2 in seen)) { seen[$2] = 1; order[++k] = $2 } }
  END { for (i = 1; i <= k; i++) { s = order[i]; printf "  %-14s %2d passed, %d failed, %d skipped\n", s, c[s, "PASS"], c[s, "FAIL"], c[s, "skip"] } }'
printf '  %d passed, %d failed, %d skipped\n' "$PASS" "$FAIL" "$SKIP"
[[ "$FAIL" == 0 ]]
