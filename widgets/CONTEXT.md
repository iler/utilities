# Context

Glossary for the terminal widget that shows AI coding-plan limits.

## Provider

An AI coding service with its own plan and its own limits. This effort covers
three providers: **Claude** (Claude Code subscription), **Codex** (ChatGPT-plan
backed Codex CLI), and **Alibaba** (Alibaba Cloud Model Studio / DashScope
token package).

A provider is not a model. One provider can serve many models under one limit.

## Limit window

A period after which a provider resets your allowance. A provider can have more
than one limit window at the same time. Claude has a **5-hour** window and a
**7-day** window, and both apply together.

## Utilization

The share of a limit window that is already spent, as a percent. `0` means
unused. `100` means the window is exhausted.

Utilization is not a token count. Providers report the percent, not the tokens.

## Reset time

The moment a limit window returns to zero utilization. The widget shows the time
left until this moment, not the moment itself.

## Reading

One provider's limit state at one point in time: its utilization per limit
window, its reset times, and the time the data was collected.

## Staleness

The age of a reading. Readings come from local caches that only refresh while
the provider's own CLI runs. A stale reading is still shown, but it is marked,
because a quiet wrong number is worse than a number marked old.

## Widget line

One line of widget output. There is one line per provider.

## Target name

The ID of a widget on your desktop. You add the widget by hand and name it.
The `terminal-widget` CLI only reaches widgets that carry the same name.

## Push

The way data gets to the widget. TerminalWidget never runs this project's code.
A scheduled job runs the code, and the code pushes a payload to a Target name.
This is the opposite of a status line, which is pulled.
