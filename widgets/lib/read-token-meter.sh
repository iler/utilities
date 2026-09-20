#!/usr/bin/env bash
# Emit a normalised token-usage Reading from the local Token Meter server.
# Token Meter serves its dashboard state on 127.0.0.1:8722. One GET /state
# call carries everything: the current session, today's totals, and the
# monthly budget rollup. The endpoint is served from cache, so it is fast.
set -uo pipefail

url="${TOKEN_METER_URL:-http://127.0.0.1:8722}/state"

payload="$(curl -s -m 10 "$url" 2>/dev/null)"
if [ -z "$payload" ]; then
  echo '{"source":"token-meter","ok":false,"note":"down","fetched_at":null}'
  exit 0
fi

# The local date must come from the shell. jq strftime is UTC, and the daily
# rows are keyed by the server's local date.
reading="$(printf '%s' "$payload" | jq -c \
  --argjson now "$(date +%s)" \
  --arg today "$(date +%F)" '
  select(.xsession != null and (.xsession.budget != null))
  | ((.xsession.daily // []) | map(select(.day == $today)) | first // null) as $day
  | {source:"token-meter", ok:true, fetched_at:$now,
     today: (if $day == null then null else {
               cost: $day.cost,
               tokens: $day.tokens,
               cost_ok: (($day.availability // {}).cost == true),
               tokens_ok: (($day.availability // {}).tokens == true)} end),
     month: (.xsession.budget | {label:.month, spend:.spend, budget:.budget,
                                 percent:.percent, configured:.configured,
                                 currency:(.currency // "USD"),
                                 projected:.projected_spend}),
     session: {provider:(.provider // null),
               model:(.primary_model // .source.model // null),
               project:((.source.project // null) | if . then (split("/") | last) else null end),
               tokens:(.total_tokens // null),
               cost:(.total_cost // null),
               cost_approx:(.cost_approx // false),
               turns:(.turns // 0),
               context_pct:((.context // {}).latest_pct // null),
               ended:(.ended // false),
               idle_s:(.idle_s // 0)}}
' 2>/dev/null)"

if [ -z "$reading" ]; then
  echo '{"source":"token-meter","ok":false,"note":"bad data","fetched_at":null}'
  exit 0
fi

printf '%s\n' "$reading"
