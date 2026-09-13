#!/usr/bin/env bash
# Claude Code statusline command. It has two jobs.
#   1. Write the documented rate_limits object to a cache the widget reads.
#      This is the only supported way to get fresh limits. The
#      cachedUsageUtilization key in ~/.claude.json goes stale for a day or more.
#   2. Print a short status line.
# To make the line invisible, replace the final printf with `printf ''`.
set -uo pipefail

cache_dir="${AI_LIMITS_CACHE:-$HOME/.cache/ai-limits}"
mkdir -p "$cache_dir"

payload="$(cat)"

# rate_limits is absent until the first API response of a session, and each
# window can be absent on its own. Write only when there is something to write.
if printf '%s' "$payload" | jq -e '.rate_limits' >/dev/null 2>&1; then
  printf '%s' "$payload" \
    | jq --argjson now "$(date +%s)" '{fetched_at: $now, rate_limits: .rate_limits}' \
    > "$cache_dir/claude.json.tmp" && mv "$cache_dir/claude.json.tmp" "$cache_dir/claude.json"
fi

five="$(printf '%s' "$payload" | jq -r '.rate_limits.five_hour.used_percentage // empty' 2>/dev/null)"
seven="$(printf '%s' "$payload" | jq -r '.rate_limits.seven_day.used_percentage // empty' 2>/dev/null)"

line=""
[ -n "$five" ]  && line="5h ${five%.*}%"
[ -n "$seven" ] && line="${line:+$line · }7d ${seven%.*}%"
printf '%s' "$line"
