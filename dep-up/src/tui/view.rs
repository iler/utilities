//! The list model: which rows the board shows, in which order, after the filter and the
//! grouping.

use crate::engine::{Activity, CheckState, Engine};
use crate::jobs::Kind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupBy {
    None,
    Lane,
    State,
}

impl GroupBy {
    pub fn next(self) -> GroupBy {
        match self {
            GroupBy::None => GroupBy::Lane,
            GroupBy::Lane => GroupBy::State,
            GroupBy::State => GroupBy::None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            GroupBy::None => "none",
            GroupBy::Lane => "lane",
            GroupBy::State => "state",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Header {
        label: String,
        jobs: Vec<usize>,
    },
    Job(usize),
    /// An outdated item from the check.
    Item {
        job: usize,
        item: usize,
    },
    /// A change of this session.
    Change {
        job: usize,
        change: usize,
    },
}

impl Row {
    pub fn job(&self) -> Option<usize> {
        match self {
            Row::Header { .. } => None,
            Row::Job(j) | Row::Item { job: j, .. } | Row::Change { job: j, .. } => Some(*j),
        }
    }
}

/// The state group of a job, with its sort key.
pub fn state_group(e: &Engine, j: usize) -> (u8, &'static str) {
    let st = &e.states[j];
    if st.missing {
        return (5, "Not installed");
    }
    if e.jobs[j].kind == Kind::Backup {
        return (3, "Backups");
    }
    match &st.check {
        CheckState::Done { items, .. } if !items.is_empty() => (0, "Outdated"),
        CheckState::Done { .. } => (4, "Up to date"),
        CheckState::Waiting if e.activity(j) != Activity::Idle || st.last.is_none() => {
            (1, "Checking")
        }
        _ => (2, "No check"),
    }
}

/// A job with the indices of its items that the filter keeps.
type Shown = (usize, Vec<usize>);
/// A group label with its jobs.
type Group = (String, Vec<Shown>);

pub fn rows(e: &Engine, filter: &str, group: GroupBy) -> Vec<Row> {
    let f = filter.trim().to_lowercase();
    // (job, item indices) for the jobs that the filter keeps.
    let mut shown: Vec<Shown> = Vec::new();
    for (j, job) in e.jobs.iter().enumerate() {
        let items = e.states[j].items();
        let job_match = f.is_empty() || job.matches(&f);
        let keep: Vec<usize> = (0..items.len())
            .filter(|&i| job_match || items[i].name.to_lowercase().contains(&f))
            .collect();
        if job_match || !keep.is_empty() {
            shown.push((j, keep));
        }
    }
    let groups: Vec<Group> = match group {
        GroupBy::None => vec![(String::new(), shown)],
        GroupBy::Lane => {
            let mut g: Vec<Group> = Vec::new();
            for s in shown {
                let lane = e.jobs[s.0].lane.to_string();
                match g.iter_mut().find(|(l, _)| *l == lane) {
                    Some((_, list)) => list.push(s),
                    None => g.push((lane, vec![s])),
                }
            }
            g
        }
        GroupBy::State => {
            let mut keyed: Vec<(u8, &str, Shown)> = shown
                .into_iter()
                .map(|s| {
                    let (k, l) = state_group(e, s.0);
                    (k, l, s)
                })
                .collect();
            keyed.sort_by_key(|(k, _, s)| (*k, s.0));
            let mut g: Vec<Group> = Vec::new();
            for (_, label, s) in keyed {
                match g.last_mut() {
                    Some((l, list)) if l == label => list.push(s),
                    _ => g.push((label.to_string(), vec![s])),
                }
            }
            g
        }
    };
    let mut out = Vec::new();
    for (label, list) in groups {
        if group != GroupBy::None {
            out.push(Row::Header {
                label,
                jobs: list.iter().map(|(j, _)| *j).collect(),
            });
        }
        for (j, items) in list {
            out.push(Row::Job(j));
            out.extend(items.into_iter().map(|item| Row::Item { job: j, item }));
            let n = e.states[j].session_changes().len();
            out.extend((0..n).map(|change| Row::Change { job: j, change }));
        }
    }
    out
}
