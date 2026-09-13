# Design: AI plan-limits widget

## Destination

A working widget on the desktop. It shows one line per provider, with the
utilization of each limit window and the time left until reset.

## Shape

A scheduled job runs every 5 minutes. It collects one Reading per provider, and
pushes a single JSON payload to TerminalWidget with `terminal-widget --json -`.
The code is bash with jq.

## Per-provider sources

| Provider | Source | Live? |
|---|---|---|
| Claude | `~/.claude.json` -> `cachedUsageUtilization` | cache only, fresh while Claude Code runs |
| Codex | `codex app-server` JSON-RPC `account/rateLimits/read` | live |
| Alibaba | `bl usage token-plan --output json` | live, needs `bailian-cli` and login |

Claude and Alibaba report percent used. Codex reports `usedPercent`. Alibaba
reports a fraction, so it must be multiplied by 100. Alibaba reset times are
epoch milliseconds. Codex reset times are epoch seconds. Claude reset times are
ISO 8601 strings.

## Display rules

- One line per provider. The line count never changes.
- Each line shows the 5-hour window and the 7-day window.
- Claude also shows extra-usage credits, but only when they are not zero.
- Thresholds are ours, and the same for all three: 70% warns, 90% is critical.
  Provider severity fields are not comparable, so we ignore them.
- A reading older than 15 minutes gets its age appended.
- An unconfigured provider shows a dash. A logged-out provider shows an auth
  warning. A line is never hidden, because a missing line reads as "fine".

## Rejected

- Polling `https://api.anthropic.com/api/oauth/usage` ourselves. It is
  undocumented and it returns 429 aggressively.
- Reading Codex session rollout files. The windows are often null.
- Alibaba Coding Plan and DashScope pay-as-you-go. Console only, no quota API.

## Findings that changed the design

- **macOS ships bash 3.2.** No `coproc`, no associative arrays. The Codex reader
  uses a FIFO. Do not add a Homebrew bash dependency, because launchd must run
  this with no special environment.
- **`cachedUsageUtilization` goes stale for a day or more**, even while Claude
  Code runs. It was 23 hours old when measured. So Claude gets a sensor: a
  `statusLine` command writes the documented `rate_limits` object to
  `~/.cache/ai-limits/claude.json` on every Claude Code turn. The reader prefers
  that cache and falls back to `cachedUsageUtilization`.
- **Alibaba reports fractions, not percents.** `per1WeekPercentage: 1` means
  100 percent used, confirmed against the text output. Multiply by 100.
- **Alibaba returns the 7-day window only.** Alibaba has lifted the 5-hour limit,
  so that window is absent. The Alibaba line is shorter than the other two.
- **`bl` writes its JSON error object to stderr**, not stdout, so the reader must
  merge the streams to tell "signed out" from "broken".
- **The nvm node directory changes with each node update.** A PATH in the plist
  that names one node version breaks after the update. The Alibaba reader
  therefore finds the newest nvm node that has `bl`, and puts that bin directory
  first in PATH, because `bl` starts with `env node`. The `up` script copies the
  global packages into a new node, so `bl` stays available.
- **The agent sandbox cannot read `~/.bailian` or launch the widget binary.**
  Alibaba and the push step can only be tested by the user. launchd is not
  sandboxed, so the scheduled job is unaffected.

## Reading

Every reader emits the same normalised object, so the renderer stays
provider-agnostic:

    {provider, ok, source, fetched_at, windows:[{label, percent, resets_at}],
     credits, currency, note}

Percent is an integer 0-100. All times are epoch seconds.

## Push flags that matter

`terminal-widget` writes a fresh payload each call and clears any option you do
not pass, so every display flag goes on every call.

- `--background-mode` is not optional for a scheduled job. Without it the CLI
  hands off through LaunchServices and can steal focus, 288 times a day.
- `--font Menlo`, because the columns are aligned with spaces.
- `--no-wrap`, because a wrapped line breaks the fixed three-line rule.
- `--timestamp`, so a dead launchd job is visible on the widget itself.

The full CLI help is saved at `docs/terminal-widget-help.txt`. The agent sandbox
cannot run the binary, so that file is the only reference available in-session.
