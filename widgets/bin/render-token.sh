#!/usr/bin/env bash
# Turn a token-usage Reading on stdin into the widget text.
# Three fixed lines on success: today, this month, current session.
# One grey line when the Reading is not ok. A hidden line reads as "fine",
# and a wrong zero reads as fact, so unavailable values render as "n/a".
set -uo pipefail

warn_at="${TOKEN_USAGE_WARN:-70}"
crit_at="${TOKEN_USAGE_CRIT:-90}"
idle_after="${TOKEN_USAGE_IDLE:-900}"
color="${TOKEN_USAGE_COLOR:-${AI_LIMITS_COLOR:-1}}"

jq -r --argjson warn "$warn_at" --argjson crit "$crit_at" \
      --argjson idle "$idle_after" --argjson color "$color" '
  # TerminalWidget flattens basic 30-37 through its theme palette, so only
  # 38;2;R;G;B survives. These are the Apple system colours, 24-bit.
  def esc(c): if $color == 1 then ([27] | implode) + "[" + c + "m" else "" end;
  def reset: esc("0");
  def red:   "1;38;2;255;69;58";
  def amber: "38;2;255;214;10";
  def green: "38;2;48;209;88";
  def grey:  "38;2;152;152;157";
  def dur:
    if . == null then ""
    elif . <= 0 then "now"
    elif . < 3600 then "\((. / 60) | floor)m"
    elif . < 86400 then "\((. / 3600) | floor)h"
    else "\((. / 86400) | floor)d" end;
  def tint(p):
    if p >= $crit then esc(red) elif p >= $warn then esc(amber) else esc(green) end;

  # Human token counts: 17331300 -> 17.3M, 914623 -> 914.6k.
  def hum:
    if . == null then "n/a"
    elif . >= 1000000 then "\((. / 100000 | round) / 10)M"
    elif . >= 1000 then "\((. / 100 | round) / 10)k"
    else "\(.)" end;

  # Fixed two-decimal money: 433.1926 -> $433.19, 0 -> $0.00.
  def usd:
    if . == null then "n/a"
    else ((if . < 0 then - . else . end) * 100 | round) as $c
      | (if . < 0 then "-$" else "$" end)
      + "\($c / 100 | floor).\($c % 100 | tostring | if length == 1 then "0" + . else . end)"
    end;

  def pad8(s): s + (" " * (8 - (s | length)));

  if .ok != true then
    esc(grey) + "token-meter " + (.note // "no data") + reset
  else
    (if .today == null then
       pad8("Today") + esc(grey) + "no sessions" + reset
     else
       pad8("Today")
       + (if .today.tokens_ok then (.today.tokens | hum) else "n/a" end) + " tok"
       + "  "
       + (if .today.cost_ok then (.today.cost | usd) else "n/a" end)
     end),
    (pad8(.month.label) + (.month.spend | usd)
     + (if .month.configured and (.month.budget // 0) > 0 then
          "  " + tint(.month.percent * 100) + "budget \((.month.percent * 100) | floor)% of \(.month.budget | usd)" + reset
          + (if .month.projected then " -> \(.month.projected | usd)" else "" end)
        else "" end)),
    (if (.session.tokens == null) and (.session.model == null) then
       pad8("Session") + esc(grey) + "none active" + reset
     else
       pad8("Session")
       + (([.session.provider, .session.model] | map(select(. != null)) | join(" ")) // "unknown")
       + "  " + (.session.tokens | hum) + " tok"
       + "  " + (if .session.cost_approx then "~" else "" end) + (.session.cost | usd)
       + "  \(.session.turns)t"
       + (if .session.context_pct then "  ctx " + tint(.session.context_pct) + "\(.session.context_pct | floor)%" + reset else "" end)
       + (if .session.ended then "  " + esc(grey) + "ended" + reset
          elif .session.idle_s >= $idle then "  " + esc(grey) + "[idle \(.session.idle_s | dur)]" + reset
          else "" end)
     end)
  end
'
