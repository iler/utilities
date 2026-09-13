#!/usr/bin/env bash
# Emit a normalised Reading for Alibaba Model Studio Token Plan.
# bl reports fractions, not percents: 1 means 100 percent used, confirmed
# against the text output. Reset times are epoch milliseconds.
# The 5-hour window is absent while Alibaba keeps that limit lifted.
set -uo pipefail

fail() { printf '{"provider":"Alibaba","ok":false,"note":"%s","windows":[],"fetched_at":null}\n' "$1"; exit 0; }

# launchd gives a bare PATH, and bl lives in an nvm node directory whose name
# changes with each node update. Use the newest nvm node that has bl. bl starts
# with "env node", so that bin directory goes first in PATH.
if ! command -v bl >/dev/null 2>&1; then
  nvm_node="${NVM_DIR:-$HOME/.nvm}/versions/node"
  for v in $(ls -1 "$nvm_node" 2>/dev/null | sort -V -r); do
    if [ -x "$nvm_node/$v/bin/bl" ]; then
      PATH="$nvm_node/$v/bin:$PATH"
      break
    fi
  done
fi

command -v bl >/dev/null 2>&1 || fail "not installed"

raw="$(bl usage token-plan --output json 2>&1)"

if [ -z "$raw" ] || printf '%s' "$raw" | jq -e '.error' >/dev/null 2>&1; then
  hint="$(printf '%s' "$raw" | jq -r '.error.message // "unreachable"' 2>/dev/null)"
  case "$hint" in
    *"console access token"*) note="auth" ;;
    *)                        note="error" ;;
  esac
  printf '{"provider":"Alibaba","ok":false,"note":"%s","windows":[],"fetched_at":null}\n' "$note"
  exit 0
fi

printf '%s' "$raw" | jq -c --argjson now "$(date +%s)" '
  {provider:"Alibaba", ok:true, source:"bl", fetched_at:$now,
   windows: ([
     (select(.per5HourPercentage != null) | {label:"5h", percent:((.per5HourPercentage * 100)|floor), resets_at:((.per5HourResetTime // 0)/1000|floor)}),
     (select(.per1WeekPercentage != null) | {label:"7d", percent:((.per1WeekPercentage * 100)|floor), resets_at:((.per1WeekResetTime // 0)/1000|floor)})
   ]),
   credits:null, currency:null, note:null}'
