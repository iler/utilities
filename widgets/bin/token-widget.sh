#!/usr/bin/env bash
# Read token usage from the local Token Meter server, render it, and push it
# to TerminalWidget. TerminalWidget never runs this script. A launchd job
# runs it, and it pushes.
#
# Set TOKEN_USAGE_DRY=1 to print the text instead of pushing.
set -uo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
# Set TW_TARGET to the widget's Edit Widget "Target name".
# Set it to an empty string to omit the flag, which makes the CLI fall back to
# the last target written, then to the first registered widget.
target="${TW_TARGET:-token-usage}"

# The reader fails soft and prints an unavailable Reading, so the widget
# always shows something.
reading="$("$here/lib/read-token-meter.sh")"

text="$(printf '%s' "$reading" | "$here/bin/render-token.sh")"

if [ "${TOKEN_USAGE_DRY:-0}" = "1" ] || ! command -v terminal-widget >/dev/null 2>&1; then
  printf '%s\n' "$text"
  exit 0
fi

# Every command writes a fresh payload and clears unspecified options, so all
# display flags must be passed every time. The reasons are the same as for
# the ai-limits widget: no focus stealing, monospace columns, fixed line
# count, and a visible timestamp when the job dies.
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
