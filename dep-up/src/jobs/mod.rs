//! The jobs. Add a job in [`registry`].
//!
//! Each job belongs to a lane. Lanes run at the same time, and the tasks of one lane run in
//! sequence. Two npm processes therefore never write to the global directory together.
//!
//! A job can have:
//! - a check, which lists the outdated items without a change,
//! - an update, for the whole job or for the items that the user selected,
//! - a snapshot, which lists the installed items for the change list,
//! - a note, which makes a short result from the output when the job has no snapshot.

pub mod brew;
pub mod gh;
pub mod macos;
pub mod mas;
pub mod node;
pub mod nono;
pub mod npm;
pub mod rust;
pub mod uv;
pub mod zsh;

use crate::run::shell::{Sh, in_path};
use crate::run::snapshot::Snapshot;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Something that a check found to be outdated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    /// What the update command needs, when it is not the name: an App Store id, a macOS
    /// update label.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default)]
    pub current: String,
    #[serde(default)]
    pub latest: String,
    /// A short remark: "pinned", "cask", "restart".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl Item {
    pub fn new(name: &str, current: &str, latest: &str) -> Item {
        Item {
            name: name.to_string(),
            id: String::new(),
            current: current.to_string(),
            latest: latest.to_string(),
            note: String::new(),
        }
    }
    pub fn id(mut self, id: &str) -> Item {
        self.id = id.to_string();
        self
    }
    pub fn note(mut self, note: &str) -> Item {
        self.note = note.to_string();
        self
    }
    /// The argument for the update command.
    pub fn arg(&self) -> &str {
        if self.id.is_empty() {
            &self.name
        } else {
            &self.id
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Update,
    /// Writes a package list. It runs by itself after the updates of its lane.
    Backup,
}

#[derive(Clone, Debug)]
pub enum Need {
    /// A program in the PATH.
    Bin(&'static str),
    /// A file that the job reads, like `~/.nvm/nvm.sh`.
    File(PathBuf),
}

impl Need {
    pub fn present(&self) -> bool {
        match self {
            Need::Bin(b) => in_path(b),
            Need::File(p) => p.exists(),
        }
    }
    /// The file name only, so a full path does not fill the board.
    pub fn label(&self) -> String {
        match self {
            Need::Bin(b) => b.to_string(),
            Need::File(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
        }
    }
}

pub type CheckFn = fn(&Ctx, &Sh) -> Result<Vec<Item>, String>;
/// `None` updates the whole job. `Some` updates only these items.
pub type UpdateFn = fn(&Ctx, &Sh, Option<&[Item]>) -> Result<(), String>;
pub type SnapFn = fn(&Ctx) -> Snapshot;
pub type NoteFn = fn(&str) -> Option<String>;

pub struct Job {
    /// A short name for the log file and the cache.
    pub id: &'static str,
    pub name: &'static str,
    pub lane: &'static str,
    pub need: Need,
    pub kind: Kind,
    /// The check step runs this job first in its lane (`brew update` before `brew outdated`).
    pub prep: bool,
    /// What the update runs, for `--list` and the details panel.
    pub command: String,
    /// Where the check gets its data, for the details panel.
    pub check_text: &'static str,
    pub check: Option<CheckFn>,
    pub update: UpdateFn,
    /// The update can do single items.
    pub per_item: bool,
    pub snapshot: Option<SnapFn>,
    pub note: Option<NoteFn>,
}

impl Job {
    pub fn base(
        id: &'static str,
        name: &'static str,
        lane: &'static str,
        need: Need,
        update: UpdateFn,
    ) -> Job {
        Job {
            id,
            name,
            lane,
            need,
            kind: Kind::Update,
            prep: false,
            command: String::new(),
            check_text: "",
            check: None,
            update,
            per_item: false,
            snapshot: None,
            note: None,
        }
    }

    /// True when `want` (lower case) is part of the name, the lane or the program.
    pub fn matches(&self, want: &str) -> bool {
        format!(
            "{} {} {} {}",
            self.name,
            self.id,
            self.lane,
            self.need.label()
        )
        .to_lowercase()
        .contains(want)
    }
}

/// Paths and settings that the jobs read. They come from the environment.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub home: PathBuf,
    pub nvm_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub skill_lock: PathBuf,
    pub zsh_plugin_dir: PathBuf,
    pub uipro_ai: Vec<String>,
    pub cargo_home: PathBuf,
}

fn env_path(name: &str, default: PathBuf) -> PathBuf {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or(default)
}

impl Ctx {
    pub fn from_env() -> Ctx {
        let home = env_path("HOME", PathBuf::from("."));
        let uipro = std::env::var("UIPRO_AI").unwrap_or_else(|_| "claude universal".into());
        Ctx {
            nvm_dir: env_path("NVM_DIR", home.join(".nvm")),
            backup_dir: env_path("DEP_UP_BACKUP_DIR", home.join(".config")),
            skill_lock: env_path("SKILL_LOCK", home.join(".agents/.skill-lock.json")),
            zsh_plugin_dir: env_path("ZSH_PLUGIN_DIR", home.join(".zsh/plugins")),
            uipro_ai: uipro.split_whitespace().map(String::from).collect(),
            cargo_home: env_path("CARGO_HOME", home.join(".cargo")),
            home,
        }
    }
    pub fn log_root(&self) -> PathBuf {
        env_path("DEP_UP_LOG_DIR", self.home.join(".local/state/dep-up"))
    }
    pub fn brewfile(&self) -> PathBuf {
        self.backup_dir.join("brew/Brewfile")
    }
    pub fn npm_list(&self) -> PathBuf {
        self.backup_dir.join("npm/packages.json")
    }
}

/// All jobs, in the order of the board. In a lane, this is also the run order.
pub fn registry(ctx: &Ctx) -> Vec<Job> {
    let mut jobs = Vec::new();
    jobs.extend(npm::jobs(ctx));
    jobs.push(node::job(ctx));
    jobs.extend(brew::jobs(ctx));
    jobs.push(macos::job());
    jobs.push(mas::job());
    jobs.push(zsh::job(ctx));
    jobs.extend(rust::jobs());
    jobs.push(uv::job());
    jobs.push(gh::job());
    jobs
}

/// The last line of the output, for a job without a snapshot.
pub fn note_last_line(text: &str) -> Option<String> {
    crate::run::tail::last_line(text)
}

/// Splits `items` by the `id`, for the commands that take a list of names.
pub fn args(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| i.arg().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_unique_ids_and_backups_after_updates() {
        let ctx = Ctx::from_env();
        let jobs = registry(&ctx);
        let mut ids: Vec<_> = jobs.iter().map(|j| j.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), jobs.len());
        for j in &jobs {
            assert!(!j.command.is_empty(), "{} has no command text", j.id);
            assert_eq!(j.check.is_some(), !j.check_text.is_empty(), "{}", j.id);
        }
    }

    /// Runs the real checks of this machine. They change nothing.
    /// `cargo test live_checks -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_checks() {
        let ctx = Ctx::from_env();
        for job in registry(&ctx) {
            let Some(check) = job.check else { continue };
            if !job.need.present() {
                println!("{:<16} not installed", job.name);
                continue;
            }
            let t = std::time::Instant::now();
            match check(&ctx, &Sh::quiet()) {
                Ok(items) => {
                    println!(
                        "{:<16} {} outdated ({:.1}s)",
                        job.name,
                        items.len(),
                        t.elapsed().as_secs_f32()
                    );
                    for i in items {
                        println!("    {} {} -> {} {}", i.name, i.current, i.latest, i.note);
                    }
                }
                Err(e) => println!("{:<16} check failed: {e}", job.name),
            }
        }
    }

    #[test]
    fn matching_uses_name_lane_and_program() {
        let ctx = Ctx::from_env();
        let jobs = registry(&ctx);
        let brew: Vec<_> = jobs
            .iter()
            .filter(|j| j.matches("brew"))
            .map(|j| j.id)
            .collect();
        assert!(brew.contains(&"brew-apps") && brew.contains(&"nono"));
        assert!(jobs.iter().any(|j| j.matches("rust") && j.id == "cargo"));
    }
}
