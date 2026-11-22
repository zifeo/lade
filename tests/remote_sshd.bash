#!/usr/bin/env bash
# A real ssh link: a Debian container runs sshd and a Linux lade, this
# machine runs `lade connect` with a fake doppler only it has. Lade state on
# this machine lives in a temp dir: the real hub, diary and config are never
# touched. The key is throwaway, no ssh agent is asked and no ssh config is
# read. Run by `cargo test --test docker` (tests/docker/remote.rs).
#
# Usage: tests/remote_sshd.bash <laptop lade binary> [linux lade binary]
# Without the second argument the laptop binary goes to the container on
# Linux, and elsewhere a Linux one is built in Docker under target/linux-docker.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
LADE=${1:?laptop lade binary, e.g. target/debug/lade}
LADE=$(cd "$(dirname "$LADE")" && pwd)/$(basename "$LADE")
if [ -n "${2:-}" ]; then
  REMOTE_LADE=$2
elif [ "$(uname -s)" = Linux ]; then
  REMOTE_LADE=$LADE
else
  echo "building a Linux lade in Docker (target/linux-docker)"
  docker run --rm -v "$ROOT:/src" -w /src -e CARGO_TARGET_DIR=/src/target/linux-docker \
    -v lade-cargo-registry:/usr/local/cargo/registry rust:1.96-bookworm \
    cargo build --release --locked --quiet
  REMOTE_LADE=$ROOT/target/linux-docker/release/lade
fi
NAME=lade-remote-sshd-$$
# Short: the ssh control socket lives under it, and macOS caps socket paths.
WORK=$(mktemp -d /tmp/lade-sshd.XXXXXX)

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

cleanup() {
  set +e
  [ -n "${SSH+x}" ] && "$LADE" detach -- "${SSH[@]}" >/dev/null 2>&1
  "$LADE" ttl forget >/dev/null 2>&1
  docker rm -f "$NAME" >/dev/null 2>&1
  rm -rf "$WORK"
}
trap cleanup EXIT

ssh-keygen -q -t ed25519 -N '' -f "$WORK/key"
cp "$REMOTE_LADE" "$WORK/lade"
cp "$WORK/key.pub" "$WORK/authorized_keys"
cat >"$WORK/ask.sh" <<'EOF'
#!/bin/bash
exec 3<>"/dev/tcp/127.0.0.1/$ECHO_PORT"
echo ping >&3
read -r line <&3
[ "$line" = ping ]
EOF
cat >"$WORK/Dockerfile" <<'EOF'
# trixie: its glibc is newer than the CI runner's, which builds the binary.
FROM debian:trixie-slim
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends openssh-server procps socat >/dev/null \
  && useradd -m -s /bin/bash me && mkdir -p /run/sshd /home/me/.ssh /home/me/src/app
COPY lade /usr/local/bin/lade
COPY authorized_keys /home/me/.ssh/authorized_keys
COPY ask.sh /home/me/ask.sh
# ECHO_PORT: an echo service the laptop reaches through its own ssh to this
# sshd, the jump host of the tunnel.
RUN printf '".":\n  TOKEN: doppler://api.doppler.com/proj/dev/TOKEN\n  ECHO_PORT: ssh://lade-sshd/127.0.0.1/7000\n' >/home/me/src/app/lade.yml \
  && chown -R me:me /home/me && chmod 700 /home/me/.ssh && chmod 600 /home/me/.ssh/authorized_keys \
  && chmod 755 /home/me/ask.sh
CMD ["sh", "-c", "socat TCP-LISTEN:7000,bind=127.0.0.1,fork,reuseaddr EXEC:cat & exec /usr/sbin/sshd -D -e"]
EOF
docker build -q -t lade-remote-sshd "$WORK" >/dev/null
docker run -d --name "$NAME" -p 127.0.0.1::22 lade-remote-sshd >/dev/null
PORT=$(docker port "$NAME" 22/tcp | head -1)
PORT=${PORT##*:}

mkdir -p "$WORK/bin"
# `lade-sshd` for the link and for the laptop's tunnel ssh alike, as an
# Orca SSH target is a ~/.ssh/config alias.
cat >"$WORK/ssh_config" <<EOF
Host lade-sshd
  HostName 127.0.0.1
  Port $PORT
  User me
  IdentityFile $WORK/key
  IdentitiesOnly yes
  IdentityAgent none
  BatchMode yes
  StrictHostKeyChecking no
  UserKnownHostsFile $WORK/known_hosts
  LogLevel ERROR
EOF
REAL_SSH=$(command -v ssh)
printf '#!/bin/sh\nexec %s -F %s "$@"\n' "$REAL_SSH" "$WORK/ssh_config" >"$WORK/bin/ssh"
chmod +x "$WORK/bin/ssh"
cat >"$WORK/bin/doppler" <<EOF
#!/bin/sh
echo called >>"$WORK/doppler-calls"
echo '{"TOKEN":{"computed":"from-the-laptop"}}'
EOF
chmod +x "$WORK/bin/doppler"
echo '{"user":null}' >"$WORK/config.json"
export PATH="$WORK/bin:$PATH"
export LADE_CACHE_DIR="$WORK/cache"
export LADE_EVENTS_PATH="$WORK/events.db"
export LADE_CONFIG_PATH="$WORK/config.json"
unset OP_SERVICE_ACCOUNT_TOKEN LADE_T LADE_VIA AI_AGENT CLAUDECODE CLAUDE_CODE CODEX_HOME || true

SSH=(ssh lade-sshd)
up=
for _ in $(seq 50); do
  if "${SSH[@]}" true 2>/dev/null; then up=1; break; fi
  sleep 0.2
done
[ -n "$up" ] || fail "sshd in the container never answered"

# `echo from-the-laptop | cksum`: Lade scrubs the value itself from the line.
CHECK='cd src/app && lade -- '\''test "$(echo "$TOKEN" | cksum)" = "1515322352 16"'\'''
remote() { "${SSH[@]}" "$@"; }
# The remote hub caches what the link served: forget it so the check crosses the link.
fresh() { remote "lade ttl forget >/dev/null && $CHECK"; }

echo "1. connect shows the walk, resolves it on the laptop and links"
"$LADE" connect src/app -- "${SSH[@]}" </dev/null 2>"$WORK/connect" \
  || fail "connect: $(cat "$WORK/connect")"
grep -q "doppler://api.doppler.com/proj/dev/TOKEN" "$WORK/connect" || fail "the view lacks the ref"

echo "3. the remote resolves through the laptop"
remote "$CHECK" || fail "remote command did not get the laptop's value"
[ -s "$WORK/doppler-calls" ] || fail "the laptop's doppler was not called"
remote 'lade status --json || true' | grep -Eq '"upstream": ?"attached"' || fail "remote status not attached"
{ "$LADE" status --json || true; } | grep -Eq '"phase": ?"ready"' || fail "laptop status has no ready link"

echo "3b. a tunnel of the remote runs on the laptop and carries the connection"
remote 'cd src/app && lade -- /home/me/ask.sh' || fail "the tunnel did not carry the connection"

echo "4. a dropped link fails with 4, then the hub brings it back"
docker exec "$NAME" pkill -f 'lade attach' || fail "no lade attach on the remote"
sleep 0.3
set +e
fresh 2>/dev/null
code=$?
set -e
[ "$code" = 4 ] || echo "   (link already back, exit $code)"
back=
for _ in $(seq 60); do
  if fresh 2>/dev/null; then back=1; break; fi
  sleep 0.5
done
[ -n "$back" ] || fail "the hub did not restart the link"

echo "5. an edit of the remote lade.yml needs a new connect: exit 6"
remote 'printf "  AGAIN: doppler://api.doppler.com/proj/dev/TOKEN\n" >>src/app/lade.yml'
set +e
fresh 2>/dev/null
code=$?
set -e
[ "$code" = 6 ] || fail "edited walk exit $code, want 6"
"$LADE" connect src/app -- "${SSH[@]}" </dev/null 2>/dev/null || fail "connect after the edit"
fresh || fail "the edited walk does not resolve after a new connect"

echo "6. detach on the laptop ends attached mode on the remote"
"$LADE" detach -- "${SSH[@]}" 2>/dev/null
for _ in $(seq 30); do
  remote 'test -e ~/.cache/lade/up/attached.json' || break
  sleep 0.2
done
remote 'test ! -e ~/.cache/lade/up/attached.json' || fail "the remote stayed attached"

echo "7. the laptop diary holds remote rows and no value"
rows=$("$LADE" log --json --kind remote --all --since 1d)
for event in connect served tunnel broken refused detach; do
  echo "$rows" | grep -Eq "\"event\": ?\"$event\"" || fail "no $event row"
done
echo "$rows" | grep -q from-the-laptop && fail "the diary holds the value"

echo "OK"
