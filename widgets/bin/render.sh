#!/usr/bin/env bash
# Turn a JSON array of Readings on stdin into the widget text.
# One line per provider, always. A hidden line reads as "fine", and that is the
# failure you cannot afford.
set -uo pipefail

warn_at="${AI_LIMITS_WARN:-70}"
crit_at="${AI_LIMITS_CRIT:-90}"
stale_after="${AI_LIMITS_STALE:-900}"
color="${AI_LIMITS_COLOR:-1}"

jq -r --argjson now "$(date +%s)" \
      --argjson warn "$warn_at" --argjson crit "$crit_at" \
      --argjson stale "$stale_after" --argjson color "$color" '
  # TerminalWidget resolves basic 30-37 through its theme palette, which flattens
  # them to the foreground colour. Confirmed on the widget: only 38;5;N and
  # 38;2;R;G;B survive. These are the Apple system colours, 24-bit.
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

  def window:
    tint(.percent) + "\(.label) \(.percent)%" + reset
    + (if .resets_at then " (\(.resets_at - $now | dur))" else "" end);

  def age:
    if .fetched_at == null then ""
    elif ($now - .fetched_at) > $stale
      then esc(grey) + " [\($now - .fetched_at | dur) old]" + reset
    else "" end;

  def credits:
    if (.credits // 0) > 0 then " + \(.currency // "") \(.credits)" else "" end;

  def body:
    if .ok != true then
      esc(grey) + (if .note == "auth" then "sign in" else (.note // "no data") end) + reset
    elif (.windows | length) == 0 then esc(grey) + "no windows" + reset
    else ([.windows[] | window] | join("  ")) + credits + age
    end;

  .[] | (.provider + (" " * (8 - (.provider | length))) + " " + body)
'
