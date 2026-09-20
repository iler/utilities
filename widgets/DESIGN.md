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

---

# Design: token-usage widget

## Destination

A second widget on the desktop. It shows token usage and spend from Token
Meter: today, this month, and the current session.

## Shape

Its own launchd job runs every 5 minutes. One reader calls the local Token
Meter server, one renderer builds three lines, and the push goes to the
Target name `token-usage`. The code is bash with jq, same as the limits
widget.

## Source

`GET http://127.0.0.1:8722/state`. No auth. Served from the server's cache,
so it answers in about 30 ms. One call carries everything:

- `xsession.daily` -> today's tokens and cost, keyed by local date.
- `xsession.budget` -> month spend, budget target, percent, projection.
- top level -> current session: provider, model, tokens, cost, turns,
  context percent, ended, idle seconds.

The local date comes from `date +%F` in the reader. jq `strftime` is UTC,
and the daily rows use the server's local date.

### Rejected sources

- `GET /menubar`: compact, but it has no daily token history and no token
  count for today.
- `GET /spend/logs` and `GET /builder-recap`: they rebuild cross-session
  state per request. Measured over 5 s, so they time out.
- The Token Meter MCP server: a stdio JSON-RPC handshake from bash is
  heavier than one curl.

## Display rules

- Three fixed lines: `Today`, the month label, `Session`.
- An unavailable value renders as `n/a`, never as zero. Token Meter marks
  availability per metric, and the reader carries those flags.
- Budget percent and context percent use the same thresholds as the limits
  widget: 70% warns, 90% is critical. They are only shown when they mean
  something: a budget percent needs a configured budget, a context percent
  needs a known context window.
- An ended session is marked `ended`. An idle session over 15 minutes shows
  its idle time. Both in grey.
- An approximate session cost gets a `~` prefix.
- When the reading is not ok, one grey line shows why: `token-meter down`
  or `token-meter bad data`.

## Failure taxonomy

- Empty curl response -> `down`. The server is not running.
- jq select fails -> `bad data`. The payload shape changed. The Token Meter
  GET API is dashboard-internal and not versioned, so an update can break
  it. `bad data` on the widget is the alarm.

## Push

Same flags as the limits widget, for the same reasons:
`--background-mode`, `--font Menlo`, `--no-wrap`, `--timestamp`,
`--ansi-mode on`. Target name `token-usage`, overridable with `TW_TARGET`.
Knobs: `TOKEN_USAGE_DRY`, `TOKEN_USAGE_WARN`, `TOKEN_USAGE_CRIT`,
`TOKEN_USAGE_IDLE`, `TOKEN_USAGE_COLOR`, `TOKEN_METER_URL`.

## Findings

- **Token Meter is installed as two LaunchAgents** (`com.token-meter.server`
  and `com.token-meter.menubar`), so the API is up at login. The widget job
  still fails soft when it is not.
- **omp sessions record cost ~$0.** Token Meter uses the cost that the
  runtime wrote to its own log and infers no price. Token counts are still
  correct, so the token-usage widget stays useful for omp.
- **The budget is unconfigured by default.** The month line then shows
  spend only. Configure a monthly budget in Token Meter settings to get the
  percent and the projection.
- **Do not read `provider_quotas` from Token Meter.** It polls the Claude
  OAuth usage API, which is the source this repo already rejected for the
  limits widget. The limits widget keeps its own readers.
