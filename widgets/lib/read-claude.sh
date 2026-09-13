#!/usr/bin/env bash
# Emit a normalised Reading for Claude.
# Prefers the statusline cache. Falls back to the stale cachedUsageUtilization
# key so the line still says something before the statusline has ever run.
set -uo pipefail

cache="${AI_LIMITS_CACHE:-$HOME/.cache/ai-limits}/claude.json"

if [ -r "$cache" ]; then
  jq -c '
    {provider:"Claude", ok:true, source:"statusline", fetched_at:.fetched_at,
     windows: ([
       (.rate_limits.five_hour  | select(. != null) | {label:"5h", percent:(.used_percentage|floor), resets_at:.resets_at}),
       (.rate_limits.seven_day  | select(. != null) | {label:"7d", percent:(.used_percentage|floor), resets_at:.resets_at})
     ]),
     credits:null, currency:null, note:null}' "$cache" 2>/dev/null && exit 0
fi

for f in "$HOME/.claude.json" "$HOME/.claude/claude.json"; do
  [ -r "$f" ] || continue
  jq -c '
    def epoch: if . == null then null
               else (sub("\\.[0-9]+";"") | sub("\\+00:00$";"Z") | fromdateiso8601) end;
    .cachedUsageUtilization as $c
    | select($c != null)
    | {provider:"Claude", ok:true, source:"cache", fetched_at: (($c.fetchedAtMs // 0) / 1000 | floor),
       windows: ([
         ($c.utilization.five_hour | select(. != null) | {label:"5h", percent:(.utilization|floor), resets_at:(.resets_at|epoch)}),
         ($c.utilization.seven_day | select(. != null) | {label:"7d", percent:(.utilization|floor), resets_at:(.resets_at|epoch)})
       ]),
       credits: ($c.utilization.extra_usage.used_credits // null),
       currency: ($c.utilization.extra_usage.currency // null),
       note:null}' "$f" 2>/dev/null && exit 0
done

echo '{"provider":"Claude","ok":false,"note":"no data","windows":[],"fetched_at":null}'
