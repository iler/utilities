//! Drawing.

use crate::dotfiles::commit::TITLE_MAX;
use crate::engine::{Activity, CheckState, Engine, RunResult};
use crate::jobs::Kind;
use crate::run::logs::{pretty, tail_lines};
use crate::run::snapshot::Mark;
use crate::style::{age, duration, glyphs};
use crate::tui::app::{App, Focus, Pick, key};
use crate::tui::view::Row;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

const CYAN: Color = Color::Cyan;
const GREY: Color = Color::DarkGray;

fn st(c: Color) -> Style {
    Style::new().fg(c)
}

fn dim() -> Style {
    Style::new().fg(Color::Gray)
}

fn spinner(app: &App) -> &'static str {
    let s = glyphs().spin;
    s[(app.started.elapsed().as_millis() / 100) as usize % s.len()]
}

/// A box with a title: cyan when it has the keys, grey otherwise. Returns the area inside.
fn panel(f: &mut Frame, area: Rect, title: &str, active: bool) -> Rect {
    let (border, title_style) = if active {
        (st(CYAN), st(CYAN).add_modifier(Modifier::BOLD))
    } else {
        (st(GREY), st(Color::Gray))
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Span::styled(format!(" {title} "), title_style));
    let inner = block.inner(area);
    f.render_widget(block, area);
    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    app.targets.keys.clear();
    app.targets.details = Rect::default();
    let area = f.area();
    let focus = app.focus();
    let width = area.width;
    let (hints, hint_keys) = footer(app, width);
    let footer_h = hints.len().max(1) as u16;
    let log_h = if app.log_open {
        (area.height / 2).max(6)
    } else {
        5
    };
    let [header, body, log, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(log_h),
        Constraint::Length(footer_h),
    ])
    .areas(area);

    f.render_widget(Paragraph::new(header_line(app)), header);

    // The list, and the details panel at the right or below it.
    let (list_area, details_area) = if !app.details {
        (body, None)
    } else if width >= 120 {
        let [l, d] = Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
            .areas(body);
        (l, Some(d))
    } else {
        // Below the list, the panel takes at most half of the height.
        let h = (body.height / 2).clamp(5, 14);
        let [l, d] = Layout::vertical([Constraint::Min(3), Constraint::Length(h)]).areas(body);
        (l, Some(d))
    };
    draw_list(
        f,
        app,
        list_area,
        focus == Focus::List || focus == Focus::Filter,
    );
    if let Some(d) = details_area {
        draw_details(f, app, d);
    }
    draw_log(f, app, log, focus == Focus::Log);

    // The footer: key hints that a click can press.
    for (y, (line, keys)) in (foot.y..).zip(hints.into_iter().zip(hint_keys)) {
        let r = Rect {
            y,
            height: 1,
            ..foot
        };
        f.render_widget(Paragraph::new(line), r);
        for (x0, w, k) in keys {
            app.targets.keys.push((Rect::new(foot.x + x0, y, w, 1), k));
        }
    }

    draw_selection(f, app);

    if app.sudo.is_some() || app.commit.is_some() {
        // Dim everything behind the dialog.
        let buf = f.buffer_mut();
        for yy in area.y..area.y + area.height {
            for xx in area.x..area.x + area.width {
                buf[(xx, yy)].modifier.insert(Modifier::DIM);
            }
        }
        app.targets.keys.clear();
        if app.sudo.is_some() {
            draw_sudo(f, app);
        } else {
            draw_commit(f, app);
        }
    }
}

fn header_line(app: &App) -> Line<'static> {
    let e = &app.engine;
    let n = e.jobs.len();
    let outdated: usize = e.states.iter().map(|s| s.items().len()).sum();
    let running = e.busy_count();
    let queued = e.queued_count();
    let mut spans = vec![Span::styled(
        " dep-up ",
        Style::new()
            .fg(Color::Black)
            .bg(CYAN)
            .add_modifier(Modifier::BOLD),
    )];
    let jobs = if n == 1 {
        "1 job".to_string()
    } else {
        format!("{n} jobs")
    };
    let mut parts = vec![jobs, format!("{outdated} outdated")];
    if running > 0 {
        parts.push(format!("{running} running"));
    }
    if queued > 0 {
        parts.push(format!("{queued} waiting"));
    }
    if let Some(oldest) = e
        .states
        .iter()
        .filter_map(|s| match s.check {
            CheckState::Done {
                at, cached: true, ..
            } => Some(at),
            _ => None,
        })
        .min()
    {
        parts.push(format!(
            "cache from {}",
            age(crate::cache::now().saturating_sub(oldest))
        ));
    }
    let busy = if running > 0 {
        format!(" {}", spinner(app))
    } else {
        String::new()
    };
    spans.push(Span::styled(
        format!("{busy}  {}", parts.join(" · ")),
        dim(),
    ));
    spans.push(Span::styled(
        format!("   group: {}", app.group.label()),
        st(GREY),
    ));
    if !app.filter.is_empty() {
        spans.push(Span::styled(
            format!("   filter: {}", app.filter),
            st(Color::Yellow),
        ));
    }
    let picked = app.picks.len();
    if picked > 0 {
        spans.push(Span::styled(format!("   {picked} selected"), st(CYAN)));
    }
    Line::from(spans)
}

/// The mark in front of a row: picked, partly picked or not.
fn mark(app: &App, row: &Row) -> (&'static str, Style) {
    let g = glyphs();
    let on = (g.on, st(CYAN));
    let off = (g.off, st(GREY));
    match row {
        Row::Header { jobs, .. } => {
            let n = jobs
                .iter()
                .filter(|j| app.picks.contains(&Pick::Job(**j)))
                .count();
            if n == 0 {
                off
            } else if n == jobs.len() {
                on
            } else {
                (g.some, st(CYAN))
            }
        }
        Row::Job(j) => {
            if app.engine.states[*j].missing {
                (" ", Style::new())
            } else if app.picks.contains(&Pick::Job(*j)) {
                on
            } else if app
                .picks
                .iter()
                .any(|p| matches!(p, Pick::Item(pj, _) if pj == j))
            {
                (g.some, st(CYAN))
            } else {
                off
            }
        }
        Row::Item { job, item } => {
            let name = app.engine.states[*job]
                .items()
                .get(*item)
                .map(|i| i.name.clone())
                .unwrap_or_default();
            if app.picks.contains(&Pick::Job(*job)) || app.picks.contains(&Pick::Item(*job, name)) {
                on
            } else {
                off
            }
        }
        Row::Change { .. } => (" ", Style::new()),
    }
}

/// The status glyph of a job and its text.
fn status(app: &App, j: usize) -> (String, Style, String, Style) {
    let g = glyphs();
    let e = &app.engine;
    let s = &e.states[j];
    let job = &e.jobs[j];
    let run_text = |r: &RunResult| {
        let mut t = r.summary();
        let n = s.items().len();
        if n > 0 && matches!(s.check, CheckState::Done { .. }) {
            let more = format!("{n} still outdated");
            t = if t.is_empty() {
                more
            } else {
                format!("{t} · {more}")
            };
        }
        t
    };
    match e.activity(j) {
        Activity::Running => {
            let t = e.tail(j);
            return (
                spinner(app).into(),
                st(CYAN),
                if t.is_empty() { "working".into() } else { t },
                dim(),
            );
        }
        Activity::Checking => return (spinner(app).into(), st(GREY), "checking".into(), st(GREY)),
        Activity::Queued => {
            return (
                g.wait.into(),
                st(CYAN),
                "waiting for its lane".into(),
                st(GREY),
            );
        }
        Activity::Idle => {}
    }
    if s.missing {
        return (
            g.miss.into(),
            st(GREY),
            format!("{} is not installed", job.need.label()),
            st(GREY),
        );
    }
    if s.skipped {
        return (
            g.skip.into(),
            st(Color::Yellow),
            "skipped after a failure in the same lane".into(),
            st(Color::Yellow),
        );
    }
    if let Some(r) = &s.last {
        return match &r.error {
            Some(err) => (g.fail.into(), st(Color::Red), err.clone(), st(Color::Red)),
            None => (g.ok.into(), st(Color::Green), run_text(r), dim()),
        };
    }
    match &s.check {
        CheckState::Done { items, cached, at } => {
            let when = if *cached {
                format!(" · cached {}", age(crate::cache::now().saturating_sub(*at)))
            } else {
                String::new()
            };
            if items.is_empty() {
                (g.ok.into(), st(GREY), format!("up to date{when}"), st(GREY))
            } else {
                (
                    g.up.into(),
                    st(Color::Yellow),
                    format!("{} outdated{when}", items.len()),
                    st(Color::Yellow),
                )
            }
        }
        CheckState::Failed(err) => (
            "!".into(),
            st(Color::Red),
            format!("check failed: {err}"),
            st(Color::Red),
        ),
        CheckState::Waiting => (
            g.wait.into(),
            st(GREY),
            "waiting for the check".into(),
            st(GREY),
        ),
        CheckState::None if job.kind == Kind::Backup => (
            g.wait.into(),
            st(GREY),
            "runs after the updates of its lane".into(),
            st(GREY),
        ),
        CheckState::None => (
            g.wait.into(),
            st(GREY),
            "no check · press i to run it".into(),
            st(GREY),
        ),
    }
}

fn elapsed(e: &Engine, j: usize) -> String {
    let s = &e.states[j];
    if let Some(t) = s.started
        && e.activity(j) == Activity::Running
    {
        return duration(t.elapsed());
    }
    s.last
        .as_ref()
        .map(|r| duration(r.elapsed))
        .unwrap_or_default()
}

fn row_line(app: &App, row: &Row, name_w: usize) -> Line<'static> {
    let g = glyphs();
    let e = &app.engine;
    let (m, ms) = mark(app, row);
    match row {
        Row::Header { label, jobs } => Line::from(vec![
            Span::styled(format!("{m} "), ms),
            Span::styled(label.clone(), Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}", jobs.len()), st(GREY)),
        ]),
        Row::Job(j) => {
            let job = &e.jobs[*j];
            let (glyph, gs, text, ts) = status(app, *j);
            Line::from(vec![
                Span::styled(format!("{m} "), ms),
                Span::styled(format!("{glyph} "), gs),
                Span::styled(
                    format!("{:<name_w$} ", job.name),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{:<6}", job.lane), st(GREY)),
                Span::styled(format!("{:>5}  ", elapsed(e, *j)), st(GREY)),
                Span::styled(text, ts),
            ])
        }
        Row::Item { job, item } => {
            let Some(i) = e.states[*job].items().get(*item) else {
                return Line::default();
            };
            let w = name_w.saturating_sub(2).max(8);
            let mut spans = vec![
                Span::raw("    "),
                Span::styled(format!("{m} "), ms),
                Span::styled(format!("{:<w$} ", i.name), Style::new()),
            ];
            if i.current.is_empty() {
                spans.push(Span::styled(i.latest.clone(), st(Color::Green)));
            } else {
                spans.push(Span::styled(i.current.clone(), st(GREY)));
                spans.push(Span::styled(format!(" {} ", g.arrow), st(GREY)));
                spans.push(Span::styled(i.latest.clone(), st(Color::Green)));
            }
            if !i.note.is_empty() {
                spans.push(Span::styled(format!("  {}", i.note), st(Color::Yellow)));
            }
            Line::from(spans)
        }
        Row::Change { job, change } => {
            let list = e.states[*job].session_changes();
            let Some(c) = list.get(*change) else {
                return Line::default();
            };
            let (mk, color) = match c.mark {
                Mark::Up => (g.up, CYAN),
                Mark::Add => (g.add, Color::Green),
                Mark::Remove => (g.del, Color::Yellow),
            };
            let w = name_w.saturating_sub(2).max(8);
            Line::from(vec![
                Span::raw("      "),
                Span::styled(format!("{mk} "), st(color)),
                Span::styled(format!("{:<w$} ", c.name), dim()),
                Span::styled(c.label(g.arrow), st(GREY)),
            ])
        }
    }
}

fn draw_list(f: &mut Frame, app: &mut App, area: Rect, active: bool) {
    let title = if app.filter_edit {
        "jobs · filter"
    } else {
        "jobs"
    };
    let inner = panel(f, area, title, active);
    app.targets.list = inner;
    let h = inner.height as usize;
    if app.cursor < app.offset {
        app.offset = app.cursor;
    } else if h > 0 && app.cursor >= app.offset + h {
        app.offset = app.cursor + 1 - h;
    }
    app.offset = app.offset.min(app.rows.len().saturating_sub(h.max(1)));
    let name_w = app
        .engine
        .jobs
        .iter()
        .map(|j| j.name.chars().count())
        .max()
        .unwrap_or(10);
    if app.rows.is_empty() {
        let msg = if app.filter.is_empty() {
            "No jobs."
        } else {
            "No job matches the filter."
        };
        f.render_widget(Paragraph::new(Span::styled(msg, st(GREY))), inner);
        return;
    }
    let lines: Vec<Line> = app
        .rows
        .iter()
        .enumerate()
        .skip(app.offset)
        .take(h)
        .map(|(i, r)| {
            let line = row_line(app, r, name_w);
            if i == app.cursor {
                line.patch_style(
                    Style::new()
                        .bg(Color::Indexed(236))
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                line
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn kv(k: &str, v: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:<9}"), st(GREY)),
        Span::raw(v.into()),
    ])
}

fn detail_lines(app: &App) -> Vec<Line<'static>> {
    let g = glyphs();
    let e = &app.engine;
    let Some(row) = app.rows.get(app.cursor) else {
        return Vec::new();
    };
    let Some(j) = row.job() else {
        if let Row::Header { label, jobs } = row {
            let names: Vec<&str> = jobs.iter().map(|j| e.jobs[*j].name).collect();
            return vec![
                Line::styled(label.clone(), Style::new().add_modifier(Modifier::BOLD)),
                Line::raw(names.join(", ")),
                Line::styled(
                    "space selects the group, i runs its jobs that have work.",
                    st(GREY),
                ),
            ];
        }
        return Vec::new();
    };
    let job = &e.jobs[j];
    let s = &e.states[j];
    let mut out = Vec::new();
    if let Row::Item { item, .. } = row
        && let Some(i) = s.items().get(*item)
    {
        out.push(Line::styled(
            i.name.clone(),
            Style::new().add_modifier(Modifier::BOLD),
        ));
        if i.current.is_empty() {
            out.push(kv("version", i.latest.clone()));
        } else {
            out.push(kv(
                "version",
                format!("{} {} {}", i.current, g.arrow, i.latest),
            ));
        }
        if !i.id.is_empty() && i.id != i.name {
            out.push(kv("id", i.id.clone()));
        }
        if !i.note.is_empty() {
            out.push(kv("note", i.note.clone()));
        }
        if !job.per_item {
            out.push(Line::styled(
                "This job updates all its items together.",
                st(GREY),
            ));
        }
        out.push(Line::raw(""));
    }
    out.push(Line::styled(
        job.name.to_string(),
        Style::new().add_modifier(Modifier::BOLD),
    ));
    out.push(kv("lane", job.lane));
    out.push(kv(
        "needs",
        format!(
            "{} ({})",
            job.need.label(),
            if s.missing { "not installed" } else { "found" }
        ),
    ));
    out.push(kv("runs", job.command.clone()));
    if job.kind == Kind::Backup {
        out.push(kv(
            "when",
            "after the updates of its lane, and when you run it",
        ));
    }
    if job.prep {
        out.push(kv(
            "when",
            "at the start of the check of its lane, and when you run it",
        ));
    }
    if job.check.is_some() {
        let state = match &s.check {
            CheckState::Done { items, at, cached } => format!(
                "{} outdated · {}{}",
                items.len(),
                age(crate::cache::now().saturating_sub(*at)),
                if *cached { " (cache)" } else { "" }
            ),
            CheckState::Failed(e) => format!("failed: {e}"),
            CheckState::Waiting => "not yet".into(),
            CheckState::None => String::new(),
        };
        out.push(kv("check", format!("{} · {state}", job.check_text)));
    } else {
        out.push(kv(
            "check",
            "none: the change list shows what the run changed",
        ));
    }
    if let Some(r) = &s.last {
        let t = match &r.error {
            None => format!("{} done in {} · {}", g.ok, duration(r.elapsed), r.summary()),
            Some(err) => format!("{} failed after {}: {err}", g.fail, duration(r.elapsed)),
        };
        out.push(kv("last run", t));
    }
    out.push(kv("log", pretty(&e.log_path(j))));
    let changes = s.session_changes();
    if !changes.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled(
            format!("Changes in this session ({})", changes.len()),
            st(GREY),
        ));
        for c in changes {
            let mk = match c.mark {
                Mark::Up => g.up,
                Mark::Add => g.add,
                Mark::Remove => g.del,
            };
            out.push(Line::raw(format!("  {mk} {} {}", c.name, c.label(g.arrow))));
        }
    }
    let tail = tail_lines(&e.log_path(j), 12);
    if !tail.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled("The end of the log", st(GREY)));
        out.extend(
            tail.into_iter()
                .map(|l| Line::styled(format!("  {l}"), dim())),
        );
    }
    out
}

fn draw_details(f: &mut Frame, app: &mut App, area: Rect) {
    let lines = detail_lines(app);
    let total = lines.len() as u16;
    let inner_h = area.height.saturating_sub(2);
    let max = total.saturating_sub(inner_h);
    app.details_scroll = app.details_scroll.min(max);
    let title = if max > 0 {
        format!(
            "details {}-{} of {total}",
            app.details_scroll + 1,
            (app.details_scroll + inner_h).min(total)
        )
    } else {
        "details".into()
    };
    let inner = panel(f, area, &title, false);
    app.targets.details = inner;
    f.render_widget(Paragraph::new(lines).scroll((app.details_scroll, 0)), inner);
}

fn log_style(l: &str) -> Style {
    let g = glyphs();
    if l.starts_with(g.ok) {
        st(Color::Green)
    } else if l.starts_with(g.fail) {
        st(Color::Red)
    } else if l.starts_with('⚠') || l.starts_with(g.skip) {
        st(Color::Yellow)
    } else {
        dim()
    }
}

fn draw_log(f: &mut Frame, app: &mut App, area: Rect, active: bool) {
    let title = if active { "log · l closes" } else { "log" };
    let inner = panel(f, area, title, active);
    app.targets.log = inner;
    let h = inner.height as usize;
    let n = app.engine.log.len();
    app.log_scroll = app.log_scroll.min(n.saturating_sub(h));
    let end = n - app.log_scroll;
    let start = end.saturating_sub(h);
    let lines: Vec<Line> = app.engine.log[start..end]
        .iter()
        .map(|l| Line::styled(l.clone(), log_style(l)))
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// A key hint: the key, a space and what it does.
fn hint(k: &str, what: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(k.to_string(), st(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {what}"), st(GREY)),
    ]
}

type HintRow = Vec<(u16, u16, KeyEvent)>;

/// Wraps hints into lines of `width`. Returns the lines and the click areas of each line.
fn wrap_hints(
    items: Vec<(Vec<Span<'static>>, KeyEvent)>,
    width: u16,
) -> (Vec<Line<'static>>, Vec<HintRow>) {
    let mut lines = Vec::new();
    let mut keys = Vec::new();
    let mut cur: Vec<Span> = vec![Span::raw(" ")];
    let mut cur_keys = Vec::new();
    let mut x: u16 = 1;
    for (spans, k) in items {
        let w: u16 = spans.iter().map(|s| s.width() as u16).sum();
        if x + w + 2 > width && cur.len() > 1 {
            lines.push(Line::from(std::mem::replace(
                &mut cur,
                vec![Span::raw(" ")],
            )));
            keys.push(std::mem::take(&mut cur_keys));
            x = 1;
        }
        cur_keys.push((x, w, k));
        cur.extend(spans);
        cur.push(Span::raw("  "));
        x += w + 2;
    }
    lines.push(Line::from(cur));
    keys.push(cur_keys);
    (lines, keys)
}

fn footer(app: &App, width: u16) -> (Vec<Line<'static>>, Vec<HintRow>) {
    if let Some(m) = &app.message {
        return (
            vec![Line::styled(format!(" {m}"), st(Color::Yellow))],
            vec![Vec::new()],
        );
    }
    let k = |c: char| key(KeyCode::Char(c));
    match app.focus() {
        Focus::Filter => (
            vec![Line::from(vec![
                Span::styled(" / ", st(CYAN).add_modifier(Modifier::BOLD)),
                Span::raw(app.filter.clone()),
                Span::styled("█", st(CYAN)),
                Span::styled("   enter keeps the filter · esc clears it", st(GREY)),
            ])],
            vec![Vec::new()],
        ),
        Focus::Log => wrap_hints(
            vec![
                (hint("↑↓", "scroll"), key(KeyCode::Up)),
                (hint("l", "close the log"), k('l')),
                (hint("q", "quit"), k('q')),
            ],
            width,
        ),
        Focus::Dialog => (Vec::new(), Vec::new()),
        Focus::List => {
            let mut items = vec![
                (hint("space", "select"), k(' ')),
                (hint("a", "all"), k('a')),
                (hint("i", "run"), k('i')),
                (hint("/", "filter"), k('/')),
                (hint("g", "group"), k('g')),
                (hint("?", "details"), k('?')),
                (hint("l", "log"), k('l')),
                (hint("c", "commit"), k('c')),
                (
                    hint("R", "check again"),
                    KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT),
                ),
                (hint("q", "quit"), k('q')),
            ];
            if !app.selection_text.is_empty() {
                items.insert(0, (hint("y", "copy the selection"), k('y')));
            }
            wrap_hints(items, width)
        }
    }
}

fn draw_selection(f: &mut Frame, app: &mut App) {
    app.selection_text.clear();
    let Some(sel) = app.selection.filter(|s| s.dragged) else {
        return;
    };
    if sel.area != app.targets.details && sel.area != app.targets.log {
        app.selection = None;
        return;
    }
    let buf = f.buffer_mut();
    let mut rows = Vec::new();
    for y in sel.area.y..sel.area.y + sel.area.height {
        let Some(cols) = sel.columns(y) else { continue };
        let mut row = String::new();
        let mut skip = 0;
        for x in cols {
            let cell = &mut buf[(x, y)];
            cell.modifier.insert(Modifier::REVERSED);
            // A wide character covers the next cell, which holds nothing of its own.
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let sym = cell.symbol().to_string();
            skip = Span::raw(sym.clone()).width().saturating_sub(1);
            row.push_str(&sym);
        }
        rows.push(row.trim_end().to_string());
    }
    app.selection_text = rows.join("\n");
}

/// A dialog in the middle of the screen. Returns the area inside.
fn dialog(f: &mut Frame, title: &str, width: u16, height: u16) -> Rect {
    let area = f.area();
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    let r = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, r);
    panel(f, r, title, true)
}

fn dialog_hints(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    items: Vec<(Vec<Span<'static>>, KeyEvent)>,
) {
    let (lines, keys) = wrap_hints(items, area.width);
    let y0 = area.y + area.height.saturating_sub(lines.len() as u16);
    for (i, (line, ks)) in lines.into_iter().zip(keys).enumerate() {
        let y = y0 + i as u16;
        f.render_widget(
            Paragraph::new(line),
            Rect {
                y,
                height: 1,
                ..area
            },
        );
        for (x0, w, k) in ks {
            app.targets.keys.push((Rect::new(area.x + x0, y, w, 1), k));
        }
    }
}

fn draw_sudo(f: &mut Frame, app: &mut App) {
    let inner = dialog(f, "macOS updates need root", 70, 10);
    let lines = vec![
        Line::raw("Most macOS updates need root, and sudo needs a password now."),
        Line::raw("The jobs get no keyboard input, so dep-up can ask only before"),
        Line::raw("the job starts. dep-up leaves the screen for the password."),
    ];
    f.render_widget(Paragraph::new(lines), inner);
    dialog_hints(
        f,
        app,
        inner,
        vec![
            (hint("s", "enter the password"), key(KeyCode::Char('s'))),
            (
                hint("n", "only updates without root"),
                key(KeyCode::Char('n')),
            ),
            (hint("esc", "cancel"), key(KeyCode::Esc)),
        ],
    );
}

fn draw_commit(f: &mut Frame, app: &mut App) {
    let Some(d) = &app.commit else { return };
    let body_lines = d.body.lines();
    let height = (body_lines.len() as u16 + 13).clamp(14, 30);
    let inner = dialog(
        f,
        &format!("commit in {}", pretty(&d.plan.repo)),
        90,
        height,
    );
    let files: Vec<String> = d
        .plan
        .files
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    let title_len = d.title.len();
    let title_style = if !d.on_body { st(CYAN) } else { st(GREY) };
    let body_style = if d.on_body { st(CYAN) } else { st(GREY) };
    let count_style = if title_len > TITLE_MAX {
        st(Color::Red)
    } else {
        st(GREY)
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Title ", title_style.add_modifier(Modifier::BOLD)),
            Span::styled(format!("{title_len}/{TITLE_MAX}"), count_style),
        ]),
        Line::raw(format!("  {}", d.title.value())),
        Line::styled("Body", body_style.add_modifier(Modifier::BOLD)),
    ];
    if body_lines.iter().all(|l| l.is_empty()) {
        lines.push(Line::styled("  (no body)", st(GREY)));
    } else {
        lines.extend(body_lines.iter().map(|l| Line::raw(format!("  {l}"))));
    }
    lines.push(Line::styled(
        format!("Files: {}", files.join(", ")),
        st(GREY),
    ));
    if d.busy {
        lines.push(Line::styled("git commit runs…", st(CYAN)));
    }
    if let Some(e) = &d.error {
        lines.extend(
            e.lines()
                .map(|l| Line::styled(l.to_string(), st(Color::Red))),
        );
    }
    f.render_widget(Paragraph::new(lines), inner);
    // The cursor of the field that has the keys.
    let (cx, cy) = if d.on_body {
        let line: String = body_lines.get(d.body.row).cloned().unwrap_or_default();
        let w = Span::raw(line.chars().take(d.body.col).collect::<String>()).width() as u16;
        (inner.x + 2 + w, inner.y + 3 + d.body.row as u16)
    } else {
        let v = d.title.value();
        let w = Span::raw(v.chars().take(d.title.cursor).collect::<String>()).width() as u16;
        (inner.x + 2 + w, inner.y + 1)
    };
    if cx < inner.x + inner.width && cy < inner.y + inner.height {
        f.set_cursor_position((cx, cy));
    }
    dialog_hints(
        f,
        app,
        inner,
        vec![
            (hint("tab", "title / body"), key(KeyCode::Tab)),
            (hint("enter", "commit (in the title)"), key(KeyCode::Enter)),
            (
                hint("ctrl+s", "commit"),
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            ),
            (hint("esc", "cancel"), key(KeyCode::Esc)),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::jobs::{Ctx, Item, Job, Need};
    use crate::run::logs::RunDir;
    use crate::run::shell::Sh;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn noop(_: &Ctx, _: &Sh, _: Option<&[Item]>) -> Result<(), String> {
        Ok(())
    }

    fn none(_: &Ctx, _: &Sh) -> Result<Vec<Item>, String> {
        Ok(Vec::new())
    }

    fn screen(app: &mut App, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| draw(f, app)).unwrap();
        let buf = t.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn app() -> App {
        let mut apps = Job::base("brew-apps", "Homebrew apps", "brew", Need::Bin("sh"), noop);
        apps.per_item = true;
        apps.command = "brew upgrade".into();
        apps.check_text = "brew outdated --json=v2";
        apps.check = Some(none);
        let mut missing = Job::base(
            "mas",
            "App Store apps",
            "mas",
            Need::Bin("dep-up-no-such-bin"),
            noop,
        );
        missing.command = "mas upgrade".into();
        let mut skills = Job::base("skills", "Claude skills", "node", Need::Bin("sh"), noop);
        skills.command = "npx skills update".into();
        let root = std::env::temp_dir().join(format!("dep-up-ui-{}", std::process::id()));
        let mut e = Engine::new(
            vec![apps, missing, skills],
            Ctx::from_env(),
            RunDir::create(&root, 2).unwrap(),
            false,
            false,
        );
        e.states[0].check = CheckState::Done {
            items: vec![
                Item::new("git", "2.43.0", "2.44.0"),
                Item::new("iterm2", "3.5.0", "3.5.1").note("cask"),
            ],
            at: crate::cache::now(),
            cached: false,
        };
        e.push_log("✔ Homebrew index  00:03  Already up-to-date.");
        App::new(e)
    }

    #[test]
    fn draws_the_board() {
        let mut a = app();
        let s = screen(&mut a, 100, 20);
        println!("{s}");
        assert!(s.contains("dep-up"));
        assert!(s.contains("Homebrew apps") && s.contains("2 outdated"));
        assert!(s.contains("git") && s.contains("2.44.0") && s.contains("cask"));
        assert!(s.contains("dep-up-no-such-bin is not installed"));
        assert!(s.contains("no check"));
        assert!(s.contains("Already up-to-date."));
        assert!(s.contains("space select"));
    }

    #[test]
    fn draws_details_and_dialogs_in_small_windows() {
        let mut a = app();
        a.details = true;
        a.cursor = 1;
        let s = screen(&mut a, 80, 30);
        println!("{s}");
        assert!(s.contains("details") && s.contains("brew outdated --json=v2"));
        a.sudo = Some(vec![(0, None)]);
        let s = screen(&mut a, 60, 12);
        assert!(s.contains("enter the password"));
        // Very small windows must not panic.
        a.sudo = None;
        for (w, h) in [(10, 5), (1, 1), (40, 8)] {
            screen(&mut a, w, h);
        }
    }
}
