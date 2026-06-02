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

TAPES=(main hooks resolution inject file-output per-user disclaimer eval)

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
    function line_pri(line,    pri) {
      pri = 99
      if (tape ~ /^main$/) {
        if (line ~ /^> echo /) pri = 1
        else if (line ~ /^> lade install/) pri = 2
        return pri
      }
      if (tape ~ /^(inject|file-output|disclaimer|eval|hooks)$/) {
        if (line ~ /^> \.\/cat-lade\.yml/) pri = 0
        else if (line ~ /^> lade inject/) pri = 1
        else if (line ~ /^> lade eval /) pri = 2
        return pri
      }
      if (tape ~ /^(resolution|per-user)$/) {
        if (line ~ /^> \.\/cat-lade\.yml/) pri = 0
        else if (line ~ /^> lade user /) pri = 1
        else if (line ~ /^> lade inject/) pri = 2
        else if (line ~ /^> lade eval /) pri = 3
        else if (line ~ /^> echo /) pri = 4
        return pri
      }
      if (line ~ /^> lade user /) pri = 1
      else if (line ~ /^> lade inject/) pri = 2
      else if (line ~ /^> lade eval /) pri = 3
      else if (line ~ /^> echo /) pri = 4
      else if (line ~ /^> lade install$/) pri = 5
      return pri
    }
    function extract_cat_lade_block(buf,    lines, i, nf, out, in_block) {
      nf = split(buf, lines, "\n")
      out = ""
      in_block = 0
      for (i = 1; i <= nf; i++) {
        if (lines[i] ~ /^> \.\/cat-lade\.yml/) in_block = 1
        if (!in_block) continue
        if (lines[i] ~ /^> lade /) break
        if (out != "") out = out "\n"
        out = out lines[i]
      }
      return out == "" ? "" : out "\n"
    }
    function sanitize(s,    t) {
      t = s
      # Any per-tape HOME under .vhs-home/<tape>/… → ~
      gsub(vhs_home_root "/[^/]+", "~", t)
      gsub(vhs_home_root, "~", t)
      gsub(tape_dir, "examples/tape", t)
      return t
    }
    # Drop Hide scrollback: keep from first post-Show demo prompt (not last > echo).
    function trim_scrollback(buf,    lines, i, start, nf, out, pri, best_pri) {
      nf = split(buf, lines, "\n")
      best_pri = 99
      for (i = 1; i <= nf; i++) {
        pri = line_pri(lines[i])
        if (pri < best_pri) {
          best_pri = pri
          start = i
        }
      }
      if (!start) return buf
      out = ""
      for (i = start; i <= nf; i++) {
        if (out != "") out = out "\n"
        out = out lines[i]
      }
      return out "\n"
    }
    function is_stderr_line(line) {
      if (line ~ /Lade loaded:/) return 1
      if (substr(line, 1, 1) == "|") return 1
      if (line ~ /Are you sure/) return 1
      if (line ~ /Type "yes"/) return 1
      if (line ~ /command failed/) return 1
      if (line ~ /Not injecting/) return 1
      return 0
    }
    function flush_frame() {
      if (buf == "") return
      nframes++
      frames[nframes] = buf
      nf = split(buf, lines, "\n")
      for (i = 1; i <= nf; i++) {
        line = lines[i]
        if (is_stderr_line(line) && !(line in stderr_seen)) {
          stderr_seen[line] = 1
          stderr_order[++stderr_n] = line
        }
      }
      buf = ""
    }
    /^────────────────────────────────────────────────────────────────────────────────$/ {
      flush_frame()
      next
    }
    { buf = buf $0 ORS }
    END {
      flush_frame()
      if (nframes < 1) exit
      body = trim_scrollback(frames[nframes])
      if (tape ~ /^(inject|file-output|disclaimer|eval|hooks|resolution|per-user)$/ && index(body, "cat-lade.yml") == 0) {
        for (fi = nframes; fi >= 1; fi--) {
          prefix = extract_cat_lade_block(frames[fi])
          if (prefix != "") {
            body = prefix body
            break
          }
        }
      }
      printf "%s", sanitize(body)
      for (i = 1; i <= stderr_n; i++) {
        line = stderr_order[i]
        if (index(body, line) == 0) {
          print sanitize(line)
        }
      }
    }
  ' "$path" > "${path}.tmp"
  if [ ! -s "${path}.tmp" ]; then
    echo "render.sh: canonicalize produced empty ${path} (see ${LOG_DIR})" >&2
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
    # One HOME per tape so parallel `lade install` does not race on the same ~/.zshrc.
    export HOME="$tape_dir/.vhs-home/$tape"
    mkdir -p "$HOME"
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

echo "render: done"
