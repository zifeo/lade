#!/usr/bin/env bash
# Print one Keep a Changelog section for a GitHub release body.
# The heading date is dropped. The compare link under the section stays,
# so the version heading remains a link.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <changelog> <version>" >&2
  exit 1
fi

file=$1
version=$2

if [[ ! -f "$file" ]]; then
  echo "changelog not found: $file" >&2
  exit 1
fi

notes="$(
  awk -v version="$version" '
    function is_heading(line, heading,    n) {
      n = length(heading)
      return length(line) >= n && substr(line, 1, n) == heading && (length(line) == n || substr(line, n + 1, 1) == " ")
    }
    BEGIN { heading = "## [" version "]" }
    is_heading($0, heading) {
      found = 1
      next
    }
    found && /^## \[/ { stop = 1 }
    stop { next }
    found { lines[++n] = $0 }
    END {
      if (!found) exit 2
      start = 1
      while (start <= n && lines[start] ~ /^[[:space:]]*$/) start++
      end = n
      while (end >= start && lines[end] ~ /^[[:space:]]*$/) end--
      if (start > end) exit 3
      for (i = start; i <= end; i++) print lines[i]
    }
  ' "$file"
)" || {
  status=$?
  if [[ "$status" -eq 2 ]]; then
    echo "changelog has no section ## [$version]" >&2
  elif [[ "$status" -eq 3 ]]; then
    echo "changelog section ## [$version] is empty" >&2
  else
    echo "failed to read changelog section ## [$version]" >&2
  fi
  exit 1
}

printf '## [%s]\n\n%s\n' "$version" "$notes"
