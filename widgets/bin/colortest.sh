#!/usr/bin/env bash
# Push a colour test card to the widget. Each line uses a different ANSI
# encoding for the same red. Whichever line looks red is the one to use.
# Run bin/widget.sh afterwards to put the real content back.
set -uo pipefail
target="${TW_TARGET-ai-limits}"
esc=$(printf '\033')
r="${esc}[0m"

{
  printf '%s[31mbasic-31%s   plain 30-37\n'        "$esc" "$r"
  printf '%s[91mbright-91%s  bright 90-97\n'       "$esc" "$r"
  printf '%s[38;5;196m256-colour%s 38;5;N\n'       "$esc" "$r"
  printf '%s[38;2;255;69;58mtruecolor%s  38;2;R;G;B\n' "$esc" "$r"
  printf '%s[1mbold%s        control line\n'       "$esc" "$r"
} > /tmp/ai-limits-colortest.txt

if [ -n "$target" ]; then set -- --target "$target"; else set --; fi
cat /tmp/ai-limits-colortest.txt | terminal-widget "$@" \
  --text - --ansi-mode on --no-wrap --font "Menlo" --font-size 13 --background-mode
