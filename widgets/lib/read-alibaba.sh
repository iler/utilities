#!/usr/bin/env bash
# Emit a normalised Reading for Alibaba Model Studio Token Plan.
# bl reports fractions, not percents: 1 means 100 percent used, confirmed
# against the text output. Reset times are epoch milliseconds.
# The 5-hour window is absent while Alibaba keeps that limit lifted.
set -uo pipefail

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
