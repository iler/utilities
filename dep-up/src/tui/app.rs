//! The state of the TUI and the key and mouse handling.

use crate::dotfiles::commit::{self, Plan};
use crate::engine::{CheckState, Engine};
use crate::jobs::{Item, Kind, macos};
use crate::tui::text::{TextArea, TextField};
use crate::tui::view::{self, GroupBy, Row};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pick {
    /// The whole job: its own update command.
    Job(usize),
    /// One outdated item of a job, by name.
    Item(usize, String),
}

pub type Batch = Vec<(usize, Option<Vec<Item>>)>;

/// Something the main loop must do outside the TUI.
pub enum Request {
    /// Leave the TUI, run `sudo -v`, come back, then run this batch.
    Sudo(Batch),
}

pub enum Msg {
    Plan(Result<Plan, String>),
    Committed(Result<String, String>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    List,
    Filter,
    Log,
    Dialog,
}

pub struct CommitDialog {
    pub plan: Plan,
    pub title: TextField,
    pub body: TextArea,
    pub body_edited: bool,
    pub on_body: bool,
    pub busy: bool,
    pub error: Option<String>,
}

/// Text that the mouse selects in the details panel or the log.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub area: Rect,
    pub start: (u16, u16),
    pub end: (u16, u16),
    pub dragged: bool,
}

impl Selection {
    /// The selected columns of row `y`, in reading order.
    pub fn columns(&self, y: u16) -> Option<std::ops::Range<u16>> {
        let (a, b) = if (self.start.1, self.start.0) <= (self.end.1, self.end.0) {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        };
        if y < a.1 || y > b.1 {
            return None;
        }
        let left = if y == a.1 { a.0 } else { self.area.x };
        let right = if y == b.1 {
            b.0 + 1
        } else {
            self.area.x + self.area.width
        };
        Some(left.max(self.area.x)..right.min(self.area.x + self.area.width))
    }
}

/// Where the last frame put things, for the mouse.
#[derive(Default)]
pub struct Targets {
    pub list: Rect,
    pub details: Rect,
    pub log: Rect,
    pub keys: Vec<(Rect, KeyEvent)>,
}

pub struct App {
    pub engine: Engine,
    pub rows: Vec<Row>,
    pub cursor: usize,
    pub offset: usize,
    pub picks: BTreeSet<Pick>,
    pub filter: String,
    pub filter_edit: bool,
    pub group: GroupBy,
    pub details: bool,
    pub details_scroll: u16,
    pub log_open: bool,
    /// Lines up from the end of the log.
    pub log_scroll: usize,
    pub commit: Option<CommitDialog>,
    pub planning: bool,
    /// A batch that waits for the sudo question.
    pub sudo: Option<Batch>,
    pub request: Option<Request>,
    pub quit_armed: bool,
    pub quit: bool,
    pub message: Option<String>,
    pub started: Instant,
    pub targets: Targets,
    pub selection: Option<Selection>,
    pub selection_text: String,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

impl App {
    pub fn new(engine: Engine) -> App {
        let (tx, rx) = mpsc::channel();
        let mut app = App {
            engine,
            rows: Vec::new(),
            cursor: 0,
            offset: 0,
            picks: BTreeSet::new(),
            filter: String::new(),
            filter_edit: false,
            group: GroupBy::None,
            details: false,
            details_scroll: 0,
            log_open: false,
            log_scroll: 0,
            commit: None,
            planning: false,
            sudo: None,
            request: None,
            quit_armed: false,
            quit: false,
            message: None,
            started: Instant::now(),
            targets: Targets::default(),
            selection: None,
            selection_text: String::new(),
            tx,
            rx,
        };
        app.refresh();
        app
    }

    pub fn focus(&self) -> Focus {
        if self.commit.is_some() || self.sudo.is_some() {
            Focus::Dialog
        } else if self.filter_edit {
            Focus::Filter
        } else if self.log_open {
            Focus::Log
        } else {
            Focus::List
        }
    }

    /// Rebuilds the rows and keeps the cursor on the same row when it can.
    pub fn refresh(&mut self) {
        let current = self.rows.get(self.cursor).cloned();
        self.rows = view::rows(&self.engine, &self.filter, self.group);
        if let Some(c) = current
            && let Some(i) = self.rows.iter().position(|r| *r == c)
        {
            self.cursor = i;
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    pub fn drain(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Plan(r) => {
                    self.planning = false;
                    match r {
                        Err(e) => self.engine.push_log(format!("⚠ Commit: {e}")),
                        Ok(plan) if plan.files.is_empty() => self
                            .engine
                            .push_log("Nothing to commit: the package lists did not change since the last commit."),
                        Ok(plan) => {
                            let title = commit::title(&plan.entries);
                            let body = commit::body(&plan.entries, &title);
                            self.commit = Some(CommitDialog {
                                plan,
                                title: TextField::new(&title),
                                body: TextArea::new(&body),
                                body_edited: false,
                                on_body: false,
                                busy: false,
                                error: None,
                            });
                        }
                    }
                }
                Msg::Committed(r) => {
                    let Some(d) = &mut self.commit else { continue };
                    d.busy = false;
                    match r {
                        Ok(hash) => {
                            let line = format!(
                                "{} Committed {hash} in {}: {}",
                                crate::style::glyphs().ok,
                                crate::run::logs::pretty(&d.plan.repo),
                                d.title.value()
                            );
                            self.commit = None;
                            self.engine.push_log(line);
                        }
                        Err(e) => d.error = Some(e),
                    }
                }
            }
        }
    }

    fn page(&self) -> usize {
        (self.targets.list.height as usize).max(1)
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let max = self.rows.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, max) as usize;
        self.details_scroll = 0;
    }

    /// The job has something to do: outdated items, or no check to say that it has none.
    pub fn has_work(&self, j: usize) -> bool {
        let (job, st) = (&self.engine.jobs[j], &self.engine.states[j]);
        // The prep job runs with the check, and the backups after the updates.
        if st.missing || job.kind == Kind::Backup || job.prep {
            return false;
        }
        match &st.check {
            CheckState::Done { items, .. } => !items.is_empty(),
            _ => true,
        }
    }

    fn shown_jobs(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Job(j) => Some(*j),
                _ => None,
            })
            .collect();
        v.sort();
        v
    }

    pub fn toggle(&mut self, row: usize) {
        let Some(r) = self.rows.get(row).cloned() else {
            return;
        };
        match r {
            Row::Header { jobs, .. } => {
                let all = jobs.iter().all(|j| self.picks.contains(&Pick::Job(*j)));
                for j in jobs {
                    if all {
                        self.unpick_job(j);
                    } else if !self.engine.states[j].missing {
                        self.picks.insert(Pick::Job(j));
                    }
                }
            }
            Row::Job(j) | Row::Change { job: j, .. } => {
                if self.picks.contains(&Pick::Job(j)) {
                    self.unpick_job(j);
                } else if !self.engine.states[j].missing {
                    self.picks
                        .retain(|p| !matches!(p, Pick::Item(pj, _) if *pj == j));
                    self.picks.insert(Pick::Job(j));
                }
            }
            Row::Item { job, item } => {
                if !self.engine.jobs[job].per_item {
                    return self.toggle_job(job);
                }
                let Some(name) = self.engine.states[job]
                    .items()
                    .get(item)
                    .map(|i| i.name.clone())
                else {
                    return;
                };
                let p = Pick::Item(job, name);
                if self.picks.contains(&Pick::Job(job)) {
                    // The whole job was picked: keep all items but this one.
                    self.picks.remove(&Pick::Job(job));
                    for i in self.engine.states[job].items().to_vec() {
                        if Pick::Item(job, i.name.clone()) != p {
                            self.picks.insert(Pick::Item(job, i.name));
                        }
                    }
                } else if !self.picks.remove(&p) {
                    self.picks.insert(p);
                }
            }
        }
    }

    fn toggle_job(&mut self, j: usize) {
        if !self.picks.remove(&Pick::Job(j)) && !self.engine.states[j].missing {
            self.picks.insert(Pick::Job(j));
        }
    }

    fn unpick_job(&mut self, j: usize) {
        self.picks.retain(|p| match p {
            Pick::Job(pj) | Pick::Item(pj, _) => *pj != j,
        });
    }

    /// `a`: all shown jobs that have work, or none.
    fn toggle_all(&mut self) {
        let shown = self.shown_jobs();
        let any = self.picks.iter().any(|p| match p {
            Pick::Job(j) | Pick::Item(j, _) => shown.contains(j),
        });
        if any {
            self.picks.retain(|p| match p {
                Pick::Job(j) | Pick::Item(j, _) => !shown.contains(j),
            });
        } else {
            for j in shown {
                if self.has_work(j) {
                    self.picks.insert(Pick::Job(j));
                }
            }
        }
    }

    /// What `i` runs: the picks among the shown rows, or else the row under the cursor.
    pub fn batch(&self) -> Batch {
        let shown = self.shown_jobs();
        let mut out = Batch::new();
        let picked: Vec<&Pick> = self
            .picks
            .iter()
            .filter(|p| match p {
                Pick::Job(j) | Pick::Item(j, _) => shown.contains(j),
            })
            .collect();
        if !picked.is_empty() {
            for &j in &shown {
                if picked.contains(&&Pick::Job(j)) {
                    out.push((j, None));
                    continue;
                }
                let items: Vec<Item> = self.engine.states[j]
                    .items()
                    .iter()
                    .filter(|i| picked.contains(&&Pick::Item(j, i.name.clone())))
                    .cloned()
                    .collect();
                if !items.is_empty() {
                    out.push((j, Some(items)));
                }
            }
            return out;
        }
        match self.rows.get(self.cursor) {
            Some(Row::Job(j)) | Some(Row::Change { job: j, .. }) => out.push((*j, None)),
            Some(Row::Item { job, item }) => {
                let items = self.engine.states[*job].items();
                match items.get(*item) {
                    Some(i) if self.engine.jobs[*job].per_item => {
                        out.push((*job, Some(vec![i.clone()])))
                    }
                    _ => out.push((*job, None)),
                }
            }
            Some(Row::Header { jobs, .. }) => {
                out.extend(
                    jobs.iter()
                        .filter(|j| self.has_work(**j))
                        .map(|j| (*j, None)),
                );
            }
            None => {}
        }
        out
    }

    fn install(&mut self) {
        let batch = self.batch();
        if batch.is_empty() {
            self.message = Some("Nothing to run here.".into());
            return;
        }
        let needs_root = batch
            .iter()
            .any(|(j, _)| self.engine.jobs[*j].id == macos::ID);
        if needs_root && !macos::sudo_ready() {
            self.sudo = Some(batch);
            return;
        }
        self.start(batch);
    }

    pub fn start(&mut self, batch: Batch) {
        let jobs: Vec<usize> = batch.iter().map(|(j, _)| *j).collect();
        self.engine.enqueue_runs(batch, true);
        self.picks.retain(|p| match p {
            Pick::Job(j) | Pick::Item(j, _) => !jobs.contains(j),
        });
    }

    /// Called by the main loop after `sudo -v`.
    pub fn sudo_done(&mut self, batch: Batch, ok: bool) {
        if !ok {
            self.engine.push_log(
                "⚠ sudo -v failed. macOS updates installs only the updates that need no root.",
            );
        }
        self.start(batch);
    }

    fn recheck(&mut self) {
        let all: Vec<usize> = (0..self.engine.jobs.len()).collect();
        self.engine.enqueue_checks(&all);
        self.engine.push_log("Check all jobs again.");
    }

    fn open_commit(&mut self) {
        if self.planning {
            return;
        }
        self.planning = true;
        let ctx = (*self.engine.ctx).clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = commit::target(&ctx).and_then(|t| commit::plan(&t));
            let _ = tx.send(Msg::Plan(r));
        });
    }

    fn copy_selection(&mut self) {
        if self.selection_text.is_empty() {
            return;
        }
        let ok = copy_to_clipboard(&self.selection_text);
        self.message = Some(if ok {
            "Copied the selected text.".into()
        } else {
            "Cannot copy: no pbcopy, wl-copy, xclip or xsel.".into()
        });
        self.selection = None;
    }

    pub fn on_key(&mut self, k: KeyEvent) {
        if k.code != KeyCode::Char('q') && k.code != KeyCode::Esc {
            self.quit_armed = false;
        }
        self.message = None;
        if self.selection.is_some() && k.code != KeyCode::Char('y') {
            self.selection = None;
        }
        match self.focus() {
            Focus::Dialog => self.dialog_key(k),
            Focus::Filter => self.filter_key(k),
            Focus::Log => self.log_key(k),
            Focus::List => self.list_key(k),
        }
        self.refresh();
    }

    fn quit_key(&mut self) {
        if self.engine.busy_count() + self.engine.queued_count() == 0 {
            self.quit = true;
        } else if self.quit_armed {
            self.engine.stop_all();
            self.quit = true;
        } else {
            self.quit_armed = true;
            self.message = Some("A job runs. Press q again to stop it and quit.".into());
        }
    }

    fn list_key(&mut self, k: KeyEvent) {
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        match k.code {
            KeyCode::Up if shift => self.details_scroll = self.details_scroll.saturating_sub(1),
            KeyCode::Down if shift => self.details_scroll = self.details_scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::Home => self.move_cursor(-(self.rows.len() as isize)),
            KeyCode::End => self.move_cursor(self.rows.len() as isize),
            KeyCode::PageUp | KeyCode::Char(',') => self.move_cursor(-(self.page() as isize)),
            KeyCode::PageDown | KeyCode::Char('.') => self.move_cursor(self.page() as isize),
            KeyCode::Char(' ') => self.toggle(self.cursor),
            KeyCode::Char('a') => self.toggle_all(),
            KeyCode::Char('i') | KeyCode::Enter => self.install(),
            KeyCode::Char('/') => self.filter_edit = true,
            KeyCode::Char('g') => {
                self.group = self.group.next();
                self.cursor = 0;
            }
            KeyCode::Char('?') => {
                self.details = !self.details;
                self.details_scroll = 0;
            }
            KeyCode::Char('l') => {
                self.log_open = true;
                self.log_scroll = 0;
            }
            KeyCode::Char('c') => self.open_commit(),
            KeyCode::Char('R') => self.recheck(),
            KeyCode::Char('y') => self.copy_selection(),
            KeyCode::Char('q') | KeyCode::Esc => self.quit_key(),
            _ => {}
        }
    }

    fn filter_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Enter => self.filter_edit = false,
            KeyCode::Esc => {
                self.filter.clear();
                self.filter_edit = false;
            }
            KeyCode::Backspace => {
                self.filter.pop();
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => self.filter.push(c),
            _ => return,
        }
        self.cursor = 0;
    }

    fn log_key(&mut self, k: KeyEvent) {
        let max = self.engine.log.len();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.log_scroll = (self.log_scroll + 1).min(max),
            KeyCode::Down | KeyCode::Char('j') => {
                self.log_scroll = self.log_scroll.saturating_sub(1)
            }
            KeyCode::PageUp | KeyCode::Char(',') => {
                self.log_scroll = (self.log_scroll + 10).min(max)
            }
            KeyCode::PageDown | KeyCode::Char('.') => {
                self.log_scroll = self.log_scroll.saturating_sub(10)
            }
            KeyCode::Char('y') => self.copy_selection(),
            KeyCode::Char('l') | KeyCode::Esc => self.log_open = false,
            KeyCode::Char('q') => self.quit_key(),
            _ => {}
        }
    }

    fn dialog_key(&mut self, k: KeyEvent) {
        if let Some(batch) = self.sudo.take() {
            match k.code {
                KeyCode::Char('s') | KeyCode::Enter => self.request = Some(Request::Sudo(batch)),
                KeyCode::Char('n') => {
                    self.engine
                        .push_log("macOS updates: install only the updates that need no root.");
                    self.start(batch);
                }
                KeyCode::Esc => {}
                _ => self.sudo = Some(batch),
            }
            return;
        }
        let Some(d) = &mut self.commit else { return };
        if d.busy {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.commit = None,
            KeyCode::Tab | KeyCode::BackTab => d.on_body = !d.on_body,
            KeyCode::Char('s') if ctrl => self.do_commit(),
            KeyCode::Enter if !d.on_body => self.do_commit(),
            _ if d.on_body => {
                match k.code {
                    KeyCode::Enter => d.body.newline(),
                    KeyCode::Backspace => d.body.backspace(),
                    KeyCode::Delete => d.body.delete(),
                    KeyCode::Left => d.body.left(),
                    KeyCode::Right => d.body.right(),
                    KeyCode::Up => d.body.up(),
                    KeyCode::Down => d.body.down(),
                    KeyCode::Home => d.body.home(),
                    KeyCode::End => d.body.end(),
                    KeyCode::Char(c) if !ctrl => d.body.insert(c),
                    _ => return,
                }
                d.body_edited = true;
            }
            _ => {
                match k.code {
                    KeyCode::Backspace => d.title.backspace(),
                    KeyCode::Delete => d.title.delete(),
                    KeyCode::Left => d.title.left(),
                    KeyCode::Right => d.title.right(),
                    KeyCode::Home => d.title.home(),
                    KeyCode::End => d.title.end(),
                    KeyCode::Char(c) if !ctrl => d.title.insert(c),
                    _ => return,
                }
                // Until the body is edited, it follows the title.
                if !d.body_edited {
                    let b = commit::body(&d.plan.entries, &d.title.value());
                    d.body.set(&b);
                }
            }
        }
    }

    fn do_commit(&mut self) {
        let Some(d) = &mut self.commit else { return };
        let title = d.title.value().trim().to_string();
        if title.is_empty() {
            d.error = Some("The title is empty.".into());
            return;
        }
        d.busy = true;
        d.error = None;
        let (repo, files, body) = (d.plan.repo.clone(), d.plan.files.clone(), d.body.value());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Committed(commit::commit(&repo, &files, &title, &body)));
        });
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((_, k)) = self
                    .targets
                    .keys
                    .iter()
                    .find(|(r, _)| r.contains(pos))
                    .cloned()
                {
                    self.on_key(k);
                    return;
                }
                self.selection = None;
                if self.focus() == Focus::Dialog {
                    return;
                }
                let t = &self.targets;
                if t.list.contains(pos) {
                    self.log_open = false;
                    self.filter_edit = false;
                    let i = self.offset + (m.row - t.list.y) as usize;
                    if i < self.rows.len() {
                        self.cursor = i;
                        self.details_scroll = 0;
                        // The mark is in the first columns of the row.
                        if m.column < t.list.x + 6 {
                            self.toggle(i);
                        }
                    }
                } else if t.log.contains(pos) || t.details.contains(pos) {
                    let area = if t.log.contains(pos) {
                        t.log
                    } else {
                        t.details
                    };
                    if t.log.contains(pos) {
                        self.log_open = true;
                    }
                    self.selection = Some(Selection {
                        area,
                        start: (m.column, m.row),
                        end: (m.column, m.row),
                        dragged: false,
                    });
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(s) = &mut self.selection {
                    let r = s.area;
                    s.end = (
                        m.column.clamp(r.x, r.x + r.width.saturating_sub(1)),
                        m.row.clamp(r.y, r.y + r.height.saturating_sub(1)),
                    );
                    s.dragged = true;
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = m.kind == MouseEventKind::ScrollDown;
                let t = &self.targets;
                if t.log.contains(pos) {
                    let max = self.engine.log.len();
                    self.log_scroll = if down {
                        self.log_scroll.saturating_sub(1)
                    } else {
                        (self.log_scroll + 1).min(max)
                    };
                } else if t.details.contains(pos) {
                    self.details_scroll = if down {
                        self.details_scroll.saturating_add(1)
                    } else {
                        self.details_scroll.saturating_sub(1)
                    };
                } else if t.list.contains(pos) && self.focus() != Focus::Dialog {
                    self.move_cursor(if down { 1 } else { -1 });
                }
            }
            _ => {}
        }
        self.refresh();
    }
}

/// Copies text with the first clipboard program that exists.
fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let tools: [(&str, &[&str]); 5] = [
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("clip.exe", &[]),
    ];
    for (bin, args) in tools {
        if !crate::run::shell::in_path(bin) {
            continue;
        }
        let Ok(mut child) = Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{Ctx, Job, Need};
    use crate::run::logs::RunDir;
    use crate::run::shell::Sh;

    fn noop(_: &Ctx, _: &Sh, _: Option<&[Item]>) -> Result<(), String> {
        Ok(())
    }

    fn app() -> App {
        let mut apps = Job::base("apps", "Apps", "a", Need::Bin("sh"), noop);
        apps.per_item = true;
        apps.command = "x".into();
        let mut other = Job::base("other", "Other", "b", Need::Bin("sh"), noop);
        other.command = "y".into();
        let mut backup = Job::base("backup", "Backup", "a", Need::Bin("sh"), noop);
        backup.kind = Kind::Backup;
        backup.command = "z".into();
        let root = std::env::temp_dir().join(format!("dep-up-app-{}", std::process::id()));
        let rundir = RunDir::create(&root, 2).unwrap();
        let mut e = Engine::new(
            vec![apps, other, backup],
            Ctx::from_env(),
            rundir,
            false,
            false,
        );
        e.states[0].check = CheckState::Done {
            items: vec![Item::new("git", "1", "2"), Item::new("jq", "1", "2")],
            at: 0,
            cached: false,
        };
        App::new(e)
    }

    #[test]
    fn rows_list_items_under_their_job() {
        let a = app();
        assert_eq!(
            a.rows,
            vec![
                Row::Job(0),
                Row::Item { job: 0, item: 0 },
                Row::Item { job: 0, item: 1 },
                Row::Job(1),
                Row::Job(2)
            ]
        );
    }

    #[test]
    fn picks_make_the_batch() {
        let mut a = app();
        // The cursor alone: the job under it.
        assert_eq!(a.batch(), vec![(0, None)]);
        a.toggle(2);
        assert_eq!(a.batch(), vec![(0, Some(vec![Item::new("jq", "1", "2")]))]);
        // Picking the job replaces its single items.
        a.toggle(0);
        a.toggle(3);
        assert_eq!(a.batch(), vec![(0, None), (1, None)]);
        // Unpicking one item of a picked job keeps the others.
        a.toggle(1);
        assert_eq!(
            a.batch(),
            vec![(0, Some(vec![Item::new("jq", "1", "2")])), (1, None)]
        );
    }

    #[test]
    fn select_all_skips_backups_and_jobs_without_work() {
        let mut a = app();
        a.toggle_all();
        assert_eq!(a.picks, BTreeSet::from([Pick::Job(0), Pick::Job(1)]));
        a.toggle_all();
        assert!(a.picks.is_empty());
    }

    #[test]
    fn the_filter_keeps_matching_items() {
        let mut a = app();
        a.filter = "jq".into();
        a.refresh();
        assert_eq!(a.rows, vec![Row::Job(0), Row::Item { job: 0, item: 1 }]);
        a.filter = "other".into();
        a.refresh();
        assert_eq!(a.rows, vec![Row::Job(1)]);
    }

    #[test]
    fn grouping_by_state_adds_headers() {
        let mut a = app();
        a.group = GroupBy::State;
        a.refresh();
        let headers: Vec<String> = a
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::Header { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(headers, vec!["Outdated", "No check", "Backups"]);
    }

    #[test]
    fn selection_columns_follow_reading_order() {
        let s = Selection {
            area: Rect::new(2, 2, 10, 5),
            start: (8, 4),
            end: (4, 2),
            dragged: true,
        };
        assert_eq!(s.columns(2), Some(4..12));
        assert_eq!(s.columns(3), Some(2..12));
        assert_eq!(s.columns(4), Some(2..9));
        assert_eq!(s.columns(5), None);
    }
}
