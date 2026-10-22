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
#   every key type (ed25519, RSA, ECDSA P-256/384/521, OpenSSH and PEM
#   containers) as plain key, encrypted key and certificate   accepted
#   ssh-agent identity           accepted
#   SFTP upload/download         byte-identical
#
# Plus one legacy server (OpenSSH 8.3 limited to SHA-1 key exchange, CBC, HMAC-SHA1,
# ssh-rsa host keys and ssh-rsa public-key signatures): refused with a pointer to
# `algo set` by default, usable once the algorithms are allowed explicitly.
# AGENT2SSH_MATRIX_LEGACY=0 skips it.
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

# Git Bash on Windows rewrites arguments that look like POSIX paths before a native
# binary sees them, which would turn the *remote* paths below (/config/..., and
# /home/<user>/... on Dropbear) into C:/Program Files/Git/... Exclude them, as
# e2e-container.sh does. On Linux and macOS this is an unused name. Not yet run on
# Windows: see docs/reports/WINDOWS-HANDOVER.md.
export MSYS2_ARG_CONV_EXCL='/config;/home/'

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLI="${AGENT2SSH_CLI:-$REPO_ROOT/src-tauri/target/release/agent2ssh}"
SERVERS="${AGENT2SSH_MATRIX_SERVERS:-openssh:8.3 openssh:8.8 openssh:9.3 openssh:9.9 openssh:10.3 dropbear}"
BASE_PORT="${AGENT2SSH_MATRIX_PORT:-22130}"
USER_NAME=tester
USER_PASSWORD=s3cret-pw
KEY_PASSPHRASE=key-pass-phrase
# label:ssh-keygen arguments. The -pem entries use the traditional PEM container.
KEYTYPES=(
  "ed25519:-t ed25519"
  "rsa3072:-t rsa -b 3072"
  "rsa3072-pem:-t rsa -b 3072 -m PEM"
  "ecdsa256:-t ecdsa -b 256"
  "ecdsa384:-t ecdsa -b 384"
  "ecdsa521:-t ecdsa -b 521"
  "ecdsa256-pem:-t ecdsa -b 256 -m PEM"
)

WORK="$(mktemp -d)"
export AGENT2SSH_CONFIG_DIR="$WORK/config"
# Passwords are stored in the app's encrypted store; a fixed master password keeps
# it unlockable across the separate CLI processes this script spawns.
export AGENT2SSH_MASTER_PASSWORD="matrix-master-pw"
mkdir -p "$AGENT2SSH_CONFIG_DIR" "$WORK/home"

PASS=0
FAIL=0
SKIP=0
RESULTS=()
note() { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
ok()   { PASS=$((PASS+1)); RESULTS+=("PASS  $CURRENT  $*"); printf '  \033[32mPASS\033[0m %s\n' "$*"; }
bad()  { FAIL=$((FAIL+1)); RESULTS+=("FAIL  $CURRENT  $*"); printf '  \033[31mFAIL\033[0m %s\n' "$*"; }
skip() { SKIP=$((SKIP+1)); RESULTS+=("skip  $CURRENT  $*"); printf '  \033[33mSKIP\033[0m %s\n' "$*"; }
# A limitation in the client, not in the fixture or the environment. Counted as
# a skip so a green run still means "everything the client supports works", but
# named in the results so it cannot quietly become invisible.
known() { SKIP=$((SKIP+1)); RESULTS+=("skip  $CURRENT  $*"); printf '  \033[33mSKIP\033[0m %s\n' "$*"; }

CONTAINER=""
cleanup() {
  # The system agent keeps a copy of the key after this script exits, so the key
  # is withdrawn before the directory holding the file is removed.
  [[ -n "${AGENT_ADDED_KEY:-}" ]] && "${NATIVE_ADD:-/bin/false}" -d "$AGENT_ADDED_KEY" >/dev/null 2>&1
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
  # One plain key, one encrypted key and one certificate-only key per type. The
  # `-pem` variants write the traditional PEM container instead of OpenSSH's, a
  # different parser in libssh2 and the only form some backends can read.
  : > "$WORK/authorized_extra"
  for spec in "${KEYTYPES[@]}"; do
    label="${spec%%:*}"; args="${spec#*:}"
    # shellcheck disable=SC2086
    ssh-keygen -q $args -N '' -C "matrix-k-$label" -f "$WORK/k_$label"
    # shellcheck disable=SC2086
    ssh-keygen -q $args -N "$KEY_PASSPHRASE" -C "matrix-e-$label" -f "$WORK/e_$label"
    # shellcheck disable=SC2086
    ssh-keygen -q $args -N '' -C "matrix-c-$label" -f "$WORK/c_$label"
    ssh-keygen -q -s "$WORK/ca" -I "matrix-c-$label" -n "$USER_NAME" -V +1h "$WORK/c_$label.pub"
    cat "$WORK/k_$label.pub" "$WORK/e_$label.pub" >> "$WORK/authorized_extra"   # not the c_ keys
  done
  AUTHORIZED_EXTRA_B64="$(base64 < "$WORK/authorized_extra" | tr -d '\n')"
  echo "  keys: app-generated ed25519 x2, encrypted ed25519, CA + 3 certificates,"
  echo "        and plain/encrypted/certificate keys for: ${KEYTYPES[*]%%:*}"
else
  echo "  note: no ssh-keygen on this host; encrypted-key and certificate cases will be skipped"
fi

# ── Helpers ──────────────────────────────────────────────────────────────────
# Does `agent2ssh exec <host>` run a command? 0 = connected and ran it.
connects() {
  local out
  # HOME points at an empty directory so the app's fallback to ~/.ssh/id_* cannot
  # turn a rejection into an acceptance with the developer's own keys.
  out="$(HOME="$WORK/home" "$CLI" exec "$1" 'echo matrix-ok' --json 2>&1)" || return 1
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
    -e UN="$USER_NAME" -e UP="$USER_PASSWORD" -e AK64="$AUTHORIZED_EXTRA_B64" \
    -p "$1:2222" alpine:3.20 sh -c '
      apk add --no-cache dropbear openssh-sftp-server >/dev/null &&
      adduser -D -s /bin/sh "$UN" && echo "$UN:$UP" | chpasswd &&
      install -d -m 700 -o "$UN" -g "$UN" /home/$UN/.ssh &&
      { printf "%s\n%s\n" "$PK" "$ENCPK"; echo "$AK64" | base64 -d; } > /home/$UN/.ssh/authorized_keys &&
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

# An agent holding the ed25519 key from the plain set, for the identity case.
#
# Which agent is a platform question, not a convenience. On Windows the app
# reaches Pageant or the Win32-OpenSSH named pipe (`\\.\pipe\openssh-ssh-agent`)
# — never a unix-socket agent — so `ssh-agent -s` here would start something the
# client under test cannot see, and the case would fail for a reason that has
# nothing to do with the code. The system agent is used instead, and it has to be
# running already: starting a Windows service is not something a test should do.
# On Unix the scripted agent is the one the app talks to, so it starts its own.
AGENT_OK=0
AGENT_ADDED_KEY=""
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    NATIVE_ADD="/c/WINDOWS/System32/OpenSSH/ssh-add.exe"
    KEY_WIN="$(cygpath -w "$WORK/k_ed25519")"
    # Win32-OpenSSH refuses a key whose ACL grants anyone but its owner, and a
    # temp directory inherits whatever ACEs the host puts there — on this machine
    # that includes a sandbox group, and `ssh-add` exits with "Bad permissions"
    # while printing an UNPROTECTED PRIVATE KEY banner. Replacing the ACL with
    # one entry for the current user is the same thing `chmod 600` achieves on
    # Unix; the directory is a throwaway.
    if [[ "$HAVE_KEYGEN" == 1 && -x "$NATIVE_ADD" ]] &&
       command -v icacls >/dev/null 2>&1 &&
       icacls "$KEY_WIN" //inheritance:r //grant:r "$USERNAME:F" >/dev/null 2>&1 &&
       "$NATIVE_ADD" "$KEY_WIN" >/dev/null 2>&1; then
      AGENT_OK=1
      AGENT_ADDED_KEY="$KEY_WIN"
    fi
    ;;
  *)
    if [[ "$HAVE_KEYGEN" == 1 ]] && command -v ssh-agent >/dev/null && command -v ssh-add >/dev/null; then
      eval "$(ssh-agent -s)" >/dev/null
      trap 'ssh-agent -k >/dev/null 2>&1; cleanup' EXIT
      ssh-add -q "$WORK/k_ed25519" 2>/dev/null && AGENT_OK=1
    fi
    ;;
esac

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
  #
  # `PerSourcePenalties no` is not incidental. OpenSSH 9.8 enabled it by
  # default, with `min:15s` — the first authentication failure blocks the source
  # address for at least fifteen seconds, and the penalties accumulate. This
  # matrix deliberately fails authentication four times in a row (wrong
  # password, unauthorized key, wrong passphrase, wrong-principal certificate)
  # from a single address, so on a default-config server from 9.8 onward every
  # *positive* case after the first negative one is dropped mid-handshake and
  # reads as `Error: Disconnected`, not as a rejection. The keyword is unknown
  # before 9.8 and an unknown keyword makes sshd refuse to start, so it is
  # added, validated with `sshd -t`, and removed again when the server does not
  # know it. The key-exchange/algorithm cases below still exercise the server's
  # real policy; only the source-address rate limiter is turned off.
  SETUP_OK=1
  if [[ "$HAVE_KEYGEN" == 1 && "$kind" == openssh ]]; then
    rt_exec sh -c "echo '$(cat "$KEY_ENC.pub")' >> /config/.ssh/authorized_keys &&
      echo '$AUTHORIZED_EXTRA_B64' | base64 -d >> /config/.ssh/authorized_keys" || SETUP_OK=0
    rt_exec sh -c "
      cfg=/config/sshd/sshd_config; [ -f \$cfg ] || cfg=/etc/ssh/sshd_config
      echo '$(cat "$WORK/ca.pub")' > /config/ca.pub && chmod 644 /config/ca.pub &&
      echo 'TrustedUserCAKeys /config/ca.pub' >> \$cfg &&
      sshd_bin=/usr/sbin/sshd; [ -x \$sshd_bin ] || sshd_bin=/usr/sbin/sshd.pam
      echo 'PerSourcePenalties no' >> \$cfg
      echo 'MaxStartups 100:30:200' >> \$cfg
      # Both lines are fixture tuning, and only `PerSourcePenalties` is
      # version-gated (9.8+). An sshd that predates it refuses the whole file,
      # so on failure both appended lines are removed — `MaxStartups` is valid
      # everywhere, so losing it costs nothing but a wider queue.
      \$sshd_bin -t -f \$cfg 2>/dev/null || { sed -i '\$ d' \$cfg; sed -i '\$ d' \$cfg; }
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

  # ssh-agent: a profile with neither key nor password falls through to the agent.
  if [[ "$AGENT_OK" == 1 ]]; then
    add_host "$idx-agent" "$port"
    expect_accept "$idx-agent" "ssh-agent identity"
  else
    skip "ssh-agent identity (no reachable agent; on Windows: Start-Service ssh-agent)"
  fi

  # SFTP round trip over the key profile (upload then download, compared).
  remote_dir=/config; [[ "$kind" == dropbear ]] && remote_dir="/home/$USER_NAME"
  # 1 MiB: past 128 KiB, where a stall against Dropbear used to show up.
  head -c 1048576 /dev/urandom > "$WORK/sftp.in"
  if "$CLI" sftp put "$idx-key" "$WORK/sftp.in" "$remote_dir/sftp.bin" >/dev/null 2>&1 &&
     "$CLI" sftp get "$idx-key" "$remote_dir/sftp.bin" "$WORK/sftp.out" >/dev/null 2>&1 &&
     cmp -s "$WORK/sftp.in" "$WORK/sftp.out"; then
    ok "SFTP upload/download byte-identical"
  else
    bad "SFTP upload/download byte-identical"
  fi
  rm -f "$WORK/sftp.out"

  # Every key type, as a plain key, an encrypted key and (OpenSSH) a certificate.
  if [[ "$HAVE_KEYGEN" == 1 ]]; then
    for spec in "${KEYTYPES[@]}"; do
      label="${spec%%:*}"
      add_host "$idx-k-$label" "$port" --key "$WORK/k_$label"
      expect_accept "$idx-k-$label" "$label key"
      # Encrypted legacy-PEM EC keys used to be a known limitation: the transport
      # decrypted the container and then parsed the plaintext as PKCS#1. They go
      # through `legacy_pem` now, so this is a normal case.
      add_host "$idx-e-$label" "$port" --key "$WORK/e_$label" --passphrase "$KEY_PASSPHRASE"
      expect_accept "$idx-e-$label" "$label key, encrypted"
      if [[ "$kind" == openssh ]]; then
        # RSA certificates used to be reported as a known limitation: the
        # transport signed them with the legacy `ssh-rsa-cert-v01` type, which
        # OpenSSH 8.8+ does not offer. They are exercised now — the transport
        # upgrade plus a hash-carrying signer is what made that possible.
        add_host "$idx-c-$label" "$port" --key "$WORK/c_$label"
        expect_accept "$idx-c-$label" "$label user certificate"
      fi
    done
  fi

  "$RT_BIN" rm -f "$CONTAINER" >/dev/null 2>&1
  CONTAINER=""
done

# ── Legacy server ────────────────────────────────────────────────────────────
if [[ "${AGENT2SSH_MATRIX_LEGACY:-1}" == 1 && "$HAVE_KEYGEN" == 1 ]]; then
  CURRENT="openssh:8.3+legacy"
  port=$((BASE_PORT + idx)); idx=$((idx + 1))
  CONTAINER="agent2ssh-matrix-legacy"
  note "$CURRENT on port $port"
  "$RT_BIN" rm -f "$CONTAINER" >/dev/null 2>&1
  if ! start_openssh 8.3 "$port"; then
    bad "start $CURRENT (image pull or run failed)"
  else
    # Wait for sshd without going through the app: the app pins the first host key
    # it sees, and that must be the legacy RSA one, not the default ed25519.
    up=0
    for _ in $(seq 1 60); do
      rt_exec test -s /config/sshd.pid >/dev/null 2>&1 && { up=1; break; }
      sleep 2
    done
    rt_exec sh -c "
      echo '$AUTHORIZED_EXTRA_B64' | base64 -d >> /config/.ssh/authorized_keys
      cfg=/config/sshd/sshd_config; [ -f \$cfg ] || cfg=/etc/ssh/sshd_config
      {
        echo 'HostKey /config/ssh_host_keys/ssh_host_rsa_key'
        echo 'HostKeyAlgorithms ssh-rsa'
        echo 'KexAlgorithms diffie-hellman-group14-sha1'
        echo 'Ciphers aes128-cbc'
        echo 'MACs hmac-sha1'
        echo 'PubkeyAcceptedKeyTypes ssh-rsa'
      } >> \$cfg
      kill -HUP \$(cat /config/sshd.pid)" >/dev/null 2>&1
    sleep 2
    add_host legacy-pw "$port" --password "$USER_PASSWORD"
    add_host legacy-rsa "$port" --key "$WORK/k_rsa3072"
    add_host legacy-ed "$port" --key "$WORK/k_ed25519"

    # Default algorithms cannot meet this server, and the failure says what to do.
    err="$(HOME="$WORK/home" "$CLI" exec legacy-pw true 2>&1)"
    if [[ $? -ne 0 && "$err" == *"algo set"* ]]; then
      ok "default algorithms are refused, with a pointer to \`algo set\`"
    else
      bad "default algorithms are refused, with a pointer to \`algo set\`"; echo "        $err" | cut -c1-200
    fi

    # `algo set` is where this stops, and that is the honest outcome: the
    # transport implements no SHA-1 key exchange at all, so a server restricted
    # to `diffie-hellman-group14-sha1` cannot be reached by allowing it. What the
    # case pins is the refusal — it must name the algorithm and list what the
    # build does offer, rather than accept the value and fail later at the
    # handshake with `No common algorithm` and no way to tell why.
    #
    # Servers offering only SHA-1 KEX are pre-OpenSSH-6.5 configurations. The
    # libssh2 backend this replaced could negotiate them; the russh backend
    # cannot. Recorded in docs/KNOWN-ISSUES.md.
    # `algo set` stores what it is given — the value is only checked when a
    # connection applies it. The refusal therefore has to be pinned at the
    # connect, where it names the algorithm and lists what this build offers.
    "$CLI" algo set --kex diffie-hellman-group14-sha1 --hostkey ssh-rsa \
      --cipher-cs aes128-cbc --cipher-sc aes128-cbc --mac-cs hmac-sha1 --mac-sc hmac-sha1 >/dev/null 2>&1
    err="$(HOME="$WORK/home" "$CLI" exec legacy-pw true 2>&1)"
    if [[ $? -ne 0 && "$err" == *"unsupported SSH algorithm"* && "$err" == *"supported:"* ]]; then
      known "a SHA-1 key exchange (diffie-hellman-group14-sha1) cannot be used: the connect names it as unsupported"
    else
      bad "a SHA-1 key exchange is refused with the supported list"; echo "        $err" | cut -c1-200
    fi
    # With no legacy preferences applied, the server must still refuse this
    # client's defaults — which is what says the refusal above was about the
    # algorithm and not about the host.
    expect_reject legacy-ed "ed25519 key is refused by a server that accepts only ssh-rsa keys"
    "$CLI" algo clear >/dev/null
    expect_reject legacy-pw "defaults apply again after \`algo clear\`"
  fi
  "$RT_BIN" rm -f "$CONTAINER" >/dev/null 2>&1
  CONTAINER=""
fi

# ── Summary ──────────────────────────────────────────────────────────────────
note "Summary"
printf '  runtime: %s (%s)\n' "${RT_ID:-unknown}" "$RT_BIN"
printf '%s\n' "${RESULTS[@]}" | awk '
  { n[$2]++; c[$2, $1]++; if (!($2 in seen)) { seen[$2] = 1; order[++k] = $2 } }
  END { for (i = 1; i <= k; i++) { s = order[i]; printf "  %-14s %2d passed, %d failed, %d skipped\n", s, c[s, "PASS"], c[s, "FAIL"], c[s, "skip"] } }'
printf '  %d passed, %d failed, %d skipped\n' "$PASS" "$FAIL" "$SKIP"
[[ "$FAIL" == 0 ]]
