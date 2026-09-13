#!/usr/bin/env bash
# Emit a normalised Reading for Codex, over the app-server JSON-RPC interface.
# Works on bash 3.2, so it needs a FIFO instead of coproc.
# primary is the 5-hour window, secondary the weekly one. Resets are epoch seconds.
set -uo pipefail

timeout_secs="${CODEX_READ_TIMEOUT:-15}"

fail() { printf '{"provider":"Codex","ok":false,"note":"%s","windows":[],"fetched_at":null}\n' "$1"; exit 0; }

command -v codex >/dev/null 2>&1 || fail "not installed"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
mkfifo "$tmpdir/in"

# fd 4 reads the server. fd 3 holds its stdin open, or it exits before it answers.
exec 4< <(codex app-server < "$tmpdir/in" 2>/dev/null)
exec 3> "$tmpdir/in"

printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"widgets","title":"widgets","version":"0.1.0"}}}' \
  '{"jsonrpc":"2.0","method":"initialized","params":{}}' \
  '{"jsonrpc":"2.0","id":2,"method":"account/rateLimits/read","params":{}}' >&3

answer=""
while IFS= read -r -t "$timeout_secs" -u 4 line; do
  case "$line" in *'"id":2'*) answer="$line"; break ;; esac
done
exec 3>&- ; exec 4<&-

[ -n "$answer" ] || fail "no answer"
printf '%s' "$answer" | jq -e '.result.rateLimits' >/dev/null 2>&1 || fail "auth"

printf '%s' "$answer" | jq -c --argjson now "$(date +%s)" '
  .result.rateLimits as $r
  | {provider:"Codex", ok:true, source:"app-server", fetched_at:$now,
     windows: ([
       ($r.primary   | select(. != null) | {label:"5h", percent:(.usedPercent|floor), resets_at:.resetsAt}),
       ($r.secondary | select(. != null) | {label:"7d", percent:(.usedPercent|floor), resets_at:.resetsAt})
     ]),
     credits:null, currency:null, note:null}'
