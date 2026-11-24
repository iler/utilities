//! Dotfiles: the chezmoi sync after the jobs, and the commit of the package lists.
//!
//! If chezmoi and a source directory exist, dep-up runs `chezmoi re-add` when all jobs are
//! done. It copies the changed dotfiles, the Brewfile and the npm package list into the
//! dotfiles repository. It never commits by itself. chezmoi never overwrites a template, so a
//! secret from a password manager cannot travel back into the repository.

pub mod commit;

use crate::run::shell::in_path;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The chezmoi source directory, if chezmoi is installed and has one.
pub fn chezmoi_source() -> Option<PathBuf> {
    if !in_path("chezmoi") {
        return None;
    }
    let out = Command::new("chezmoi")
        .arg("source-path")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    p.is_dir().then_some(p)
}

/// Runs `chezmoi re-add`. `None` without chezmoi. The output goes to `log`.
pub fn sync(log: &Path) -> Option<Result<PathBuf, String>> {
    let src = chezmoi_source()?;
    let file = match std::fs::File::create(log) {
        Ok(f) => f,
        Err(e) => return Some(Err(format!("cannot write {}: {e}", log.display()))),
    };
    let err = match file.try_clone() {
        Ok(f) => f,
        Err(e) => return Some(Err(e.to_string())),
    };
    let status = Command::new("chezmoi")
        .arg("re-add")
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(err)
        .status();
    Some(match status {
        Ok(s) if s.success() => Ok(src),
        _ => Err(format!(
            "chezmoi re-add failed. See {}",
            crate::run::logs::pretty(log)
        )),
    })
}
