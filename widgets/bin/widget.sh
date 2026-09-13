#!/usr/bin/env bash
# Collect one Reading per provider, render them, and push them to TerminalWidget.
# TerminalWidget never runs this script. A launchd job runs it, and it pushes.
#
# Set AI_LIMITS_DRY=1 to print the text instead of pushing.
set -uo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
# Set TW_TARGET to the widget's Edit Widget "Target name".
# Set it to an empty string to omit the flag, which makes the CLI fall back to
# the last target written, then to the first registered widget.
target="${TW_TARGET-ai-limits}"

# Readers must not block the job. Each one already fails soft and prints an
# unavailable Reading, so the line count never changes.
readings="$(
  { "$here/lib/read-claude.sh"
    "$here/lib/read-codex.sh"
    "$here/lib/read-alibaba.sh"
  } | jq -s -c '.'
)"

text="$(printf '%s' "$readings" | "$here/bin/render.sh")"

if [ "${AI_LIMITS_DRY:-0}" = "1" ] || ! command -v terminal-widget >/dev/null 2>&1; then
  printf '%s\n' "$text"
  exit 0
fi

# Every command writes a fresh payload and clears unspecified options, so all
# display flags must be passed every time.
#   --background-mode  no LaunchServices handoff, so a 5-minute job never steals focus
#   --font Menlo       the columns are aligned with spaces, so they need a monospace font
#   --no-wrap          keeps the line count at three, which the display rules depend on
#   --timestamp        shows when the job last ran, so a dead job is visible
if [ -n "$target" ]; then
  set -- --target "$target"
else
  set --
fi

printf '%s' "$text" | terminal-widget "$@" \
  --text - \
  --ansi-mode on \
  --no-wrap \
  --font "Menlo" \
  --font-size 13 \
  --timestamp \
  --background-mode
