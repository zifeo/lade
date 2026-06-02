#!/usr/bin/env bash
# Re-record README GIFs and golden .txt transcripts (requires vhs, ttyd, ffmpeg, lade, docker).
#
# Uses the `lade` binary on your PATH (from `cargo install --path ../..`). Not target/debug.

set -euo pipefail

DISPLAY_W=640
DISPLAY_H=320

tape_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$tape_dir/../.." && pwd)"
cd "$tape_dir"

# macOS sed -i leaves .!pid!file debris if interrupted; remove before/after render.
rm -f .!*!*.txt .!*!*.gif 2>/dev/null || true

COMPOSE_FILE="$repo_root/compose.yml"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-lade}"
LOG_DIR="$tape_dir/.render-logs"

TAPES=()
for tape in *.tape; do
  if [[ "$tape" == settings*.tape ]]; then continue; fi
  TAPES+=("${tape%.tape}")
done

if ! command -v lade >/dev/null 2>&1; then
  echo "render.sh: lade not on PATH — install with: cargo install --path ${repo_root}" >&2
  exit 1
fi

# VHS .txt is a frame log; keep the last frame plus stderr lines from earlier frames.
canonicalize_vhs_txt() {
  local path="$1"
  local tape="${1%.txt}"
  tape="${tape##*/}"
  local vhs_home_root="$tape_dir/.vhs-home"
  
  awk -v tape="$tape" -v vhs_home_root="$vhs_home_root" -v tape_dir="$tape_dir" '
    function sanitize(s,    t) {
      t = s
      gsub(vhs_home_root "/[^/]+", "~", t)
      gsub(vhs_home_root, "~", t)
      gsub(tape_dir, "examples/tape", t)
      return t
    }
    /^────────────────────────────────────────────────────────────────────────────────$/ {
      if (buf != "") last_frame = buf
      buf = ""
      next
    }
    {
      buf = buf $0 ORS
    }
    END {
      if (buf != "") last_frame = buf
      
      n = split(last_frame, lines, "\n")
      
      # Find the first line that is a prompt (starts with >)
      start = 1
      while (start <= n && lines[start] !~ /^> /) {
        start++
      }
      
      # Reconstruct body
      body = ""
      for (i = start; i < n; i++) {
        body = body lines[i] "\n"
      }
      
      printf "%s", sanitize(body)
    }
  ' "$path" > "${path}.tmp"
  
  if [ ! -s "${path}.tmp" ]; then
    echo "render.sh: canonicalize produced empty ${path}" >&2
    rm -f "${path}.tmp"
    return 1
  fi
  mv "${path}.tmp" "$path"
}

downscale_gif() {
  local path="$1"
  local w="${2:-$DISPLAY_W}"
  local h="${3:-$DISPLAY_H}"
  local tmp
  tmp="$(mktemp "${path%.gif}.XXXXXX.gif")"
  ffmpeg -y -loglevel error -i "$path" \
    -vf "scale=${w}:${h}:flags=lanczos,split[s0][s1];[s0]palettegen=stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=5" \
    -loop 0 "$tmp"
  mv "$tmp" "$path"
}

prepare_vault() {
  vault_exec() {
    docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" exec -T \
      -e VAULT_ADDR=http://127.0.0.1:8200 \
      -e VAULT_TOKEN=token \
      vault vault "$@"
  }

  docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" up -d vault >/dev/null 2>&1

  local i
  for i in $(seq 1 60); do
    if vault_exec status >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done
  vault_exec status >/dev/null 2>&1 || {
    echo "render.sh: vault not ready on http://127.0.0.1:8200" >&2
    exit 1
  }

  vault_exec kv put secret/password \
    value1=itsasecret \
    value2=itsanotsecret \
    multiline=$'a\nb' >/dev/null
  vault_exec kv put secret/org/team value=secret >/dev/null
}

render_tape() {
  local tape="$1"
  local log="$LOG_DIR/${tape}.log"
  mkdir -p "$LOG_DIR"
  if (
    export HOME="$tape_dir/.vhs-home/$tape"
    rm -rf "$HOME"
    mkdir -p "$HOME"
    echo 'unsetopt PROMPT_SP; PROMPT="> "' > "$HOME/.zshrc"
    if [ "$tape" = eval ]; then
      export VAULT_ADDR=http://127.0.0.1:8200
      export VAULT_TOKEN=token
      export LADE_VAULT_HTTP=1
    fi
    vhs -q "${tape}.tape"
    canonicalize_vhs_txt "${tape}.txt"
    if [ "$tape" = main ]; then
      downscale_gif "${tape}.gif" 800 320
    else
      downscale_gif "${tape}.gif"
    fi
  ) >"$log" 2>&1; then
    return 0
  fi
  return 1
}

echo "render: using $(command -v lade)" >&2
echo "render: preparing vault for eval…" >&2
prepare_vault

# Parallel: separate vhs processes, separate output files, separate HOME per tape (see render_tape).
# Do not share one .vhs-home — concurrent `lade install` races on ~/.zshrc.
# Sequential was a wrong guess; sed -i debris and parent lade.yml regex clashes were the real bugs.
echo "render: ${#TAPES[@]} tapes in parallel (logs: examples/tape/.render-logs/)"

pids=()
names=()
for tape in "${TAPES[@]}"; do
  render_tape "$tape" &
  pids+=($!)
  names+=("$tape")
done

failed=0
i=0
for pid in "${pids[@]}"; do
  tape="${names[$i]}"
  if wait "$pid"; then
    echo "render: ${tape} ok"
  else
    echo "render: ${tape} failed (see examples/tape/.render-logs/${tape}.log)" >&2
    failed=1
  fi
  i=$((i + 1))
done

if [ "$failed" -ne 0 ]; then
  exit 1
fi

rm -f .!*!*.txt .!*!*.gif 2>/dev/null || true
rm -rf "$tape_dir/.vhs-home"
rm -rf "$LOG_DIR"

echo "render: done"
