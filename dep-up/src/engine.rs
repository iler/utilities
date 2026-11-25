//! The engine: job states, the lane queues and the worker threads.
//!
//! Each lane has a queue of tasks. A lane runs one task at a time on its own thread, and the
//! lanes run at the same time. When a run fails, the engine drops the other runs that wait in
//! that lane: `brew upgrade` never runs after a failed `brew update`.
//!
//! The TUI and the `--all` mode both drive the engine: they put tasks in the queues, call
//! [`Engine::pump`] to start the tasks of idle lanes and apply the results that come back.

use crate::cache::CacheFile;
use crate::dotfiles;
use crate::jobs::{Ctx, Item, Job, Kind};
use crate::run::logs::RunDir;
use crate::run::shell::Live;
use crate::run::snapshot::{Change, Snapshot, diff};
use crate::run::task::{self, Done, Task, Work};
use crate::style::{duration, glyphs};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum CheckState {
    /// The job has no check.
    None,
    /// Not checked yet.
    Waiting,
    Done {
        items: Vec<Item>,
        /// Unix seconds.
        at: u64,
        /// The result came from the cache file.
        cached: bool,
    },
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct RunResult {
    pub error: Option<String>,
    pub elapsed: Duration,
    /// `None` when the job has no snapshot.
    pub changes: Option<Vec<Change>>,
    pub note: Option<String>,
}

impl RunResult {
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }
    /// "3 changed", "already up to date", the note of the job, or nothing.
    pub fn summary(&self) -> String {
        match &self.changes {
            Some(c) if !c.is_empty() => format!("{} changed", c.len()),
            Some(_) => "already up to date".into(),
            None => self.note.clone().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct JobState {
    pub missing: bool,
    pub check: CheckState,
    pub last: Option<RunResult>,
    /// The last run was dropped after a failure in the same lane.
    pub skipped: bool,
    /// When the task that runs now started.
    pub started: Option<Instant>,
    first_before: Option<Snapshot>,
    last_after: Option<Snapshot>,
}

impl JobState {
    /// What changed in this session: the first snapshot against the last one.
    pub fn session_changes(&self) -> Vec<Change> {
        match (&self.first_before, &self.last_after) {
            (Some(b), Some(a)) if !b.is_empty() && !a.is_empty() => diff(b, a),
            _ => Vec::new(),
        }
    }
    pub fn items(&self) -> &[Item] {
        match &self.check {
            CheckState::Done { items, .. } => items,
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Queued,
    Checking,
    Running,
}

enum Msg {
    Task { lane: String, done: Done },
    Dotfiles(Option<Result<PathBuf, String>>),
}

pub struct Engine {
    pub jobs: Arc<Vec<Job>>,
    pub ctx: Arc<Ctx>,
    pub states: Vec<JobState>,
    live: Vec<Live>,
    pub rundir: RunDir,
    /// The messages of the session, oldest first.
    pub log: Vec<String>,
    pub sequential: bool,
    /// Copy the output of each job to the standard output (`--all --verbose`).
    pub echo: bool,
    queues: BTreeMap<String, VecDeque<Task>>,
    busy: BTreeMap<String, Task>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// A run finished after the last dotfiles sync.
    dirty: bool,
    syncing: bool,
    stopping: bool,
    pub started: Instant,
    cache: Option<CacheFile>,
    /// The result of the last dotfiles sync.
    pub dotfiles: Option<Result<PathBuf, String>>,
}

impl Engine {
    pub fn new(jobs: Vec<Job>, ctx: Ctx, rundir: RunDir, sequential: bool, echo: bool) -> Engine {
        let states = jobs
            .iter()
            .map(|j| JobState {
                missing: !j.need.present(),
                check: if j.check.is_some() {
                    CheckState::Waiting
                } else {
                    CheckState::None
                },
                last: None,
                skipped: false,
                started: None,
                first_before: None,
                last_after: None,
            })
            .collect();
        let (tx, rx) = mpsc::channel();
        Engine {
            live: jobs.iter().map(|_| Live::default()).collect(),
            jobs: Arc::new(jobs),
            ctx: Arc::new(ctx),
            states,
            rundir,
            log: Vec::new(),
            sequential,
            echo,
            queues: BTreeMap::new(),
            busy: BTreeMap::new(),
            tx,
            rx,
            dirty: false,
            syncing: false,
            stopping: false,
            started: Instant::now(),
            cache: None,
            dotfiles: None,
        }
    }

    pub fn push_log(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
    }

    /// Reads the check results from a cache file. Jobs with a result there are not checked.
    pub fn use_cache(&mut self, path: PathBuf) {
        let (cache, warning) = CacheFile::open(path);
        if let Some(w) = warning {
            self.push_log(format!("⚠ {w}"));
        }
        for (i, job) in self.jobs.iter().enumerate() {
            if job.check.is_none() {
                continue;
            }
            if let Some(e) = cache.get(job.id) {
                self.states[i].check = CheckState::Done {
                    items: e.items.clone(),
                    at: e.checked_at,
                    cached: true,
                };
            }
        }
        self.cache = Some(cache);
    }

    pub fn lane(&self, job: usize) -> String {
        if self.sequential {
            "all".into()
        } else {
            self.jobs[job].lane.to_string()
        }
    }

    pub fn tail(&self, job: usize) -> String {
        self.live[job].tail()
    }

    pub fn log_path(&self, job: usize) -> PathBuf {
        self.rundir.log_path(self.jobs[job].id)
    }

    pub fn activity(&self, job: usize) -> Activity {
        if let Some(t) = self.busy.values().find(|t| t.job == job) {
            return match t.work {
                Work::Check => Activity::Checking,
                Work::Run(_) => Activity::Running,
            };
        }
        if self.queues.values().any(|q| q.iter().any(|t| t.job == job)) {
            Activity::Queued
        } else {
            Activity::Idle
        }
    }

    pub fn busy_count(&self) -> usize {
        self.busy.len()
    }
    pub fn queued_count(&self) -> usize {
        self.queues.values().map(|q| q.len()).sum()
    }
    /// Nothing runs, nothing waits, and the dotfiles sync is done.
    pub fn idle(&self) -> bool {
        self.busy.is_empty() && self.queued_count() == 0 && !self.syncing
    }
    /// A job ran. With `count_prep` false, a prep job that the check ran does not count.
    pub fn any_run(&self, count_prep: bool) -> bool {
        self.states
            .iter()
            .zip(self.jobs.iter())
            .any(|(s, j)| s.last.is_some() && (count_prep || !j.prep))
    }
    pub fn any_failed(&self) -> bool {
        self.states
            .iter()
            .any(|s| s.last.as_ref().is_some_and(|r| !r.ok()))
    }

    /// Checks these jobs. The prep jobs of their lanes (`brew update`) run first.
    pub fn enqueue_checks(&mut self, which: &[usize]) {
        let mut by_lane: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for &j in which {
            let st = &self.states[j];
            if self.jobs[j].check.is_none() || st.missing || self.activity(j) == Activity::Checking
            {
                continue;
            }
            let lane = self.lane(j);
            if self
                .queues
                .get(&lane)
                .is_some_and(|q| q.iter().any(|t| t.job == j && t.work == Work::Check))
            {
                continue;
            }
            by_lane.entry(lane).or_default().push(j);
        }
        for (lane, list) in by_lane {
            let preps: Vec<usize> = (0..self.jobs.len())
                .filter(|&p| self.jobs[p].prep && self.lane(p) == lane && !self.states[p].missing)
                .filter(|&p| self.activity(p) == Activity::Idle)
                .collect();
            let q = self.queues.entry(lane).or_default();
            for p in preps {
                q.push_back(Task {
                    job: p,
                    work: Work::Run(None),
                });
            }
            for j in list {
                q.push_back(Task {
                    job: j,
                    work: Work::Check,
                });
            }
        }
    }

    /// Runs these jobs: `None` for the whole job, `Some` for the selected items. The backup
    /// jobs of each lane with an update join the batch, so the package lists hold the new
    /// state. With `recheck`, each job with a check is checked again after its run.
    pub fn enqueue_runs(&mut self, batch: Vec<(usize, Option<Vec<Item>>)>, recheck: bool) {
        let mut batch: Vec<(usize, Option<Vec<Item>>)> = batch
            .into_iter()
            .filter(|(j, _)| {
                if self.states[*j].missing {
                    let msg = format!(
                        "{} {}: {} is not installed",
                        glyphs().miss,
                        self.jobs[*j].name,
                        self.jobs[*j].need.label()
                    );
                    self.push_log(msg);
                    false
                } else {
                    true
                }
            })
            .collect();
        let update_lanes: Vec<String> = batch
            .iter()
            .filter(|(j, _)| self.jobs[*j].kind == Kind::Update && !self.jobs[*j].prep)
            .map(|(j, _)| self.lane(*j))
            .collect();
        for b in 0..self.jobs.len() {
            if self.jobs[b].kind == Kind::Backup
                && !self.states[b].missing
                && update_lanes.contains(&self.lane(b))
                && !batch.iter().any(|(j, _)| *j == b)
            {
                batch.push((b, None));
            }
        }
        batch.sort_by_key(|(j, _)| *j);
        batch.dedup_by_key(|(j, _)| *j);
        for (j, items) in batch {
            let lane = self.lane(j);
            let has_check = self.jobs[j].check.is_some();
            let q = self.queues.entry(lane).or_default();
            // A task that waits for the same job is now out of date.
            q.retain(|t| t.job != j);
            q.push_back(Task {
                job: j,
                work: Work::Run(items),
            });
            if recheck && has_check {
                q.push_back(Task {
                    job: j,
                    work: Work::Check,
                });
            }
            self.states[j].skipped = false;
        }
    }

    /// Starts the next task of each idle lane, and the dotfiles sync when all is done.
    pub fn pump(&mut self) {
        if self.stopping {
            return;
        }
        let idle: Vec<String> = self
            .queues
            .iter()
            .filter(|(lane, q)| !q.is_empty() && !self.busy.contains_key(*lane))
            .map(|(lane, _)| lane.clone())
            .collect();
        for lane in idle {
            if let Some(task) = self.queues.get_mut(&lane).and_then(|q| q.pop_front()) {
                self.start(lane, task);
            }
        }
        if self.busy.is_empty() && self.queued_count() == 0 && self.dirty && !self.syncing {
            self.syncing = true;
            self.dirty = false;
            let tx = self.tx.clone();
            let log = self.rundir.path.join("dotfiles.log");
            thread::spawn(move || {
                let _ = tx.send(Msg::Dotfiles(dotfiles::sync(&log)));
            });
        }
    }

    fn start(&mut self, lane: String, task: Task) {
        let j = task.job;
        self.states[j].started = Some(Instant::now());
        let live = self.live[j].clone();
        live.reset();
        if self.echo && matches!(task.work, Work::Run(_)) {
            println!("\n==> {}", self.jobs[j].name);
        }
        let (jobs, ctx, tx, echo) = (
            self.jobs.clone(),
            self.ctx.clone(),
            self.tx.clone(),
            self.echo,
        );
        let log = self.log_path(j);
        let work = task.work.clone();
        let l = lane.clone();
        thread::spawn(move || {
            let done = task::execute(j, &jobs[j], &ctx, &log, live, echo, work);
            let _ = tx.send(Msg::Task { lane: l, done });
        });
        self.busy.insert(lane, task);
    }

    /// Applies the results that arrived. Returns true when something changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(m) = self.rx.try_recv() {
            self.apply(m);
            changed = true;
        }
        changed
    }

    /// Waits up to `d` for a result, then applies all that arrived.
    pub fn wait(&mut self, d: Duration) -> bool {
        match self.rx.recv_timeout(d) {
            Ok(m) => {
                self.apply(m);
                self.poll();
                true
            }
            Err(_) => false,
        }
    }

    fn apply(&mut self, m: Msg) {
        let g = glyphs();
        match m {
            Msg::Dotfiles(r) => {
                self.syncing = false;
                match &r {
                    Some(Ok(src)) => self.push_log(format!(
                        "{} Dotfiles: chezmoi re-add is done. Check {} and commit.",
                        g.ok,
                        crate::run::logs::pretty(src)
                    )),
                    Some(Err(e)) => self.push_log(format!("⚠ Dotfiles: {e}")),
                    None => {}
                }
                if r.is_some() {
                    self.dotfiles = r;
                }
            }
            Msg::Task { lane, done } => {
                self.busy.remove(&lane);
                match done {
                    Done::Check { job, result } => {
                        self.states[job].started = None;
                        self.states[job].check = match result {
                            Ok(items) => {
                                let at = crate::cache::now();
                                if let Some(c) = &mut self.cache {
                                    c.put(self.jobs[job].id, at, &items);
                                }
                                CheckState::Done {
                                    items,
                                    at,
                                    cached: false,
                                }
                            }
                            Err(e) => {
                                if e != "stopped" {
                                    let msg =
                                        format!("⚠ {}: the check failed: {e}", self.jobs[job].name);
                                    self.push_log(msg);
                                }
                                CheckState::Failed(e)
                            }
                        };
                    }
                    Done::Run {
                        job,
                        result,
                        before,
                        after,
                        note,
                        elapsed,
                    } => self.finish_run(lane, job, result, before, after, note, elapsed),
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_run(
        &mut self,
        lane: String,
        job: usize,
        result: Result<(), String>,
        before: Option<Snapshot>,
        after: Option<Snapshot>,
        note: Option<String>,
        elapsed: Duration,
    ) {
        let g = glyphs();
        let changes = match (&before, &after) {
            (Some(b), Some(a)) if !b.is_empty() && !a.is_empty() => Some(diff(b, a)),
            _ => None,
        };
        let name = self.jobs[job].name;
        let st = &mut self.states[job];
        st.started = None;
        if st.first_before.is_none() {
            st.first_before = before;
        }
        if after.is_some() {
            st.last_after = after;
        }
        let run = RunResult {
            error: result.err(),
            elapsed,
            changes,
            note,
        };
        let line = match &run.error {
            None => format!("{} {name}  {}  {}", g.ok, duration(elapsed), run.summary()),
            Some(e) => format!(
                "{} {name} failed: {e}. Log: {}",
                g.fail,
                crate::run::logs::pretty(&self.log_path(job))
            ),
        };
        let failed = !run.ok();
        self.states[job].last = Some(run);
        self.push_log(line.trim_end().to_string());
        if !self.jobs[job].prep {
            self.dirty = true;
        }
        if failed && !self.stopping {
            let dropped: Vec<usize> = self
                .queues
                .get_mut(&lane)
                .map(|q| {
                    let mut d = Vec::new();
                    q.retain(|t| {
                        let run = matches!(t.work, Work::Run(_));
                        if run {
                            d.push(t.job);
                        }
                        !run
                    });
                    d
                })
                .unwrap_or_default();
            for d in dropped {
                self.states[d].skipped = true;
                let msg = format!(
                    "{} {}: skipped after a failure in the same lane",
                    g.skip, self.jobs[d].name
                );
                self.push_log(msg);
            }
        }
    }

    /// Stops the running commands and drops all waiting tasks.
    pub fn stop_all(&mut self) {
        self.stopping = true;
        for q in self.queues.values_mut() {
            q.clear();
        }
        for t in self.busy.values() {
            self.live[t.job].stop();
        }
    }

    /// Waits a short time for the stopped tasks and the dotfiles sync, so the summary has their
    /// results.
    pub fn drain_stopped(&mut self, max: Duration) {
        let end = Instant::now() + max;
        while (!self.busy.is_empty() || self.syncing) && Instant::now() < end {
            self.wait(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{Need, UpdateFn};
    use crate::run::shell::{Sh, cmd};

    fn ok(_: &Ctx, sh: &Sh, _: Option<&[Item]>) -> Result<(), String> {
        sh.run(cmd("true", &[]))
    }
    fn fail(_: &Ctx, _: &Sh, _: Option<&[Item]>) -> Result<(), String> {
        Err("boom".into())
    }
    fn found(_: &Ctx, _: &Sh) -> Result<Vec<Item>, String> {
        Ok(vec![Item::new("x", "1", "2")])
    }

    fn job(id: &'static str, lane: &'static str, kind: Kind, update: UpdateFn) -> Job {
        let mut j = Job::base(id, id, lane, Need::Bin("sh"), update);
        j.kind = kind;
        j.command = id.into();
        j
    }

    fn engine(jobs: Vec<Job>) -> Engine {
        let root = std::env::temp_dir().join(format!(
            "dep-up-engine-{}-{}",
            std::process::id(),
            jobs[0].id
        ));
        let rundir = RunDir::create(&root, 2).unwrap();
        Engine::new(jobs, Ctx::from_env(), rundir, false, false)
    }

    fn run_until_idle(e: &mut Engine) {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            e.pump();
            e.wait(Duration::from_millis(20));
            if e.busy_count() == 0 && e.queued_count() == 0 {
                return;
            }
        }
        panic!("the engine did not finish");
    }

    #[test]
    fn backups_join_the_lane_of_an_update() {
        let mut e = engine(vec![
            job("a-up", "a", Kind::Update, ok),
            job("a-backup", "a", Kind::Backup, ok),
            job("b-up", "b", Kind::Update, ok),
        ]);
        e.enqueue_runs(vec![(0, None)], false);
        assert_eq!(e.activity(1), Activity::Queued);
        assert_eq!(e.activity(2), Activity::Idle);
        run_until_idle(&mut e);
        assert!(e.states[0].last.as_ref().unwrap().ok());
        assert!(e.states[1].last.as_ref().unwrap().ok());
        assert!(e.states[2].last.is_none());
    }

    #[test]
    fn a_failure_skips_the_rest_of_the_lane_only() {
        let mut e = engine(vec![
            job("c-fail", "c", Kind::Update, fail),
            job("c-next", "c", Kind::Update, ok),
            job("d-other", "d", Kind::Update, ok),
        ]);
        e.enqueue_runs(vec![(0, None), (1, None), (2, None)], false);
        run_until_idle(&mut e);
        assert_eq!(
            e.states[0].last.as_ref().unwrap().error.as_deref(),
            Some("boom")
        );
        assert!(e.states[1].skipped && e.states[1].last.is_none());
        assert!(e.states[2].last.as_ref().unwrap().ok());
        assert!(e.any_failed());
    }

    #[test]
    fn checks_run_after_the_prep_job_and_after_a_run() {
        let mut prep = job("e-index", "e", Kind::Update, ok);
        prep.prep = true;
        let mut checked = job("e-apps", "e", Kind::Update, ok);
        checked.check = Some(found);
        checked.check_text = "test";
        let mut e = engine(vec![prep, checked]);
        e.enqueue_checks(&[1]);
        assert_eq!(e.activity(0), Activity::Queued);
        run_until_idle(&mut e);
        assert!(e.states[0].last.is_some());
        assert_eq!(e.states[1].items().len(), 1);
        e.enqueue_runs(vec![(1, Some(vec![Item::new("x", "1", "2")]))], true);
        run_until_idle(&mut e);
        assert!(e.states[1].last.is_some());
        assert!(matches!(
            e.states[1].check,
            CheckState::Done { cached: false, .. }
        ));
    }
}
