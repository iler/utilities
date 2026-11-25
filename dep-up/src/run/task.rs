//! One task of a lane: a check or a run of a job. It runs on its own thread.

use crate::jobs::{Ctx, Item, Job};
use crate::run::shell::{Live, Sh};
use crate::run::snapshot::Snapshot;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Work {
    /// Update the job: `None` for all of it, `Some` for these items.
    Run(Option<Vec<Item>>),
    Check,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub job: usize,
    pub work: Work,
}

pub enum Done {
    Check {
        job: usize,
        result: Result<Vec<Item>, String>,
    },
    Run {
        job: usize,
        result: Result<(), String>,
        before: Option<Snapshot>,
        after: Option<Snapshot>,
        note: Option<String>,
        elapsed: Duration,
    },
}

pub fn execute(
    index: usize,
    job: &Job,
    ctx: &Ctx,
    log: &Path,
    live: Live,
    echo: bool,
    work: Work,
) -> Done {
    let t0 = Instant::now();
    let sh = match Sh::new(Some(log), live, echo) {
        Ok(sh) => sh,
        Err(e) => {
            let msg = format!("cannot write the log {}: {e}", log.display());
            return match work {
                Work::Check => Done::Check {
                    job: index,
                    result: Err(msg),
                },
                Work::Run(_) => Done::Run {
                    job: index,
                    result: Err(msg),
                    before: None,
                    after: None,
                    note: None,
                    elapsed: t0.elapsed(),
                },
            };
        }
    };
    match work {
        Work::Check => {
            sh.note(&format!("\n==> check: {}", job.check_text));
            let result = match job.check {
                Some(check) => check(ctx, &sh),
                None => Ok(Vec::new()),
            };
            match &result {
                Ok(items) => sh.note(&format!("==> {} outdated", items.len())),
                Err(e) => sh.note(&format!("==> check failed: {e}")),
            }
            Done::Check { job: index, result }
        }
        Work::Run(items) => {
            match &items {
                Some(list) => sh.note(&format!(
                    "\n==> {}: {}",
                    job.name,
                    list.iter()
                        .map(|i| i.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                None => sh.note(&format!("\n==> {}: {}", job.name, job.command)),
            }
            let before = job.snapshot.map(|f| f(ctx));
            let result = (job.update)(ctx, &sh, items.as_deref());
            let after = job.snapshot.map(|f| f(ctx));
            if let Err(e) = &result {
                sh.note(&format!("==> failed: {e}"));
            }
            let has_snapshots = before.as_ref().is_some_and(|s| !s.is_empty())
                && after.as_ref().is_some_and(|s| !s.is_empty());
            let note = if has_snapshots {
                None
            } else {
                job.note.and_then(|f| f(&sh.captured()))
            };
            Done::Run {
                job: index,
                result,
                before,
                after,
                note,
                elapsed: t0.elapsed(),
            }
        }
    }
}
