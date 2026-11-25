//! zsh plugins: each git clone below the plugin directory.

use super::{Ctx, Item, Job, Need};
use crate::run::shell::{Sh, capture, cmd};
use crate::run::snapshot::Snapshot;
use std::path::{Path, PathBuf};

pub fn job(ctx: &Ctx) -> Job {
    let mut j = Job::base("zsh", "zsh plugins", "zsh", Need::Bin("git"), update);
    j.command = format!(
        "git pull --ff-only in each clone in {}",
        crate::run::logs::pretty(&ctx.zsh_plugin_dir)
    );
    j.check_text = "git fetch, then the commits behind the upstream";
    j.check = Some(check);
    j.per_item = true;
    j.snapshot = Some(snapshot);
    j
}

fn plugins(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join(".git").exists())
        .collect();
    v.sort();
    v
}

fn name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The first 7 characters of a commit. git can print a longer short hash.
fn short(dir: &Path, rev: &str) -> Option<String> {
    let out = capture(
        &cmd("git", &["-C"])
            .arg(dir.display().to_string())
            .args(["rev-parse", rev]),
    )?;
    let h: String = out.trim().chars().take(7).collect();
    (!h.is_empty()).then_some(h)
}

fn git(dir: &Path, args: &[&str]) -> crate::run::shell::Cmd {
    cmd("git", &["-C"])
        .arg(dir.display().to_string())
        .args(args.iter().copied())
}

fn check(ctx: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    let mut failed = Vec::new();
    for dir in plugins(&ctx.zsh_plugin_dir) {
        let n = name(&dir);
        if !sh.output(git(&dir, &["fetch", "--quiet"]))?.ok() {
            failed.push(n);
            continue;
        }
        let out = sh.output(git(&dir, &["rev-list", "--count", "HEAD..@{u}"]))?;
        let behind: u32 = out.stdout.trim().parse().unwrap_or(0);
        if behind > 0 {
            let cur = short(&dir, "HEAD").unwrap_or_default();
            let new = short(&dir, "@{u}").unwrap_or_default();
            let note = if behind == 1 {
                "1 commit".to_string()
            } else {
                format!("{behind} commits")
            };
            items.push(Item::new(&n, &cur, &new).note(&note));
        }
    }
    if !failed.is_empty() {
        sh.say(&format!("git fetch failed for: {}", failed.join(", ")));
    }
    Ok(items)
}

/// A failed plugin does not stop the other plugins, but the job fails at the end.
fn update(ctx: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    let dir = &ctx.zsh_plugin_dir;
    if !dir.is_dir() {
        sh.say(&format!("No directory {}", dir.display()));
        return Ok(());
    }
    let mut failed = Vec::new();
    for p in plugins(dir) {
        let n = name(&p);
        if let Some(items) = items
            && !items.iter().any(|i| i.arg() == n)
        {
            continue;
        }
        sh.say(&n);
        if sh.run(git(&p, &["pull", "--ff-only"])).is_err() {
            if sh.stopped() {
                return Err("stopped".into());
            }
            failed.push(n);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("git pull failed for: {}", failed.join(", ")))
    }
}

fn snapshot(ctx: &Ctx) -> Snapshot {
    plugins(&ctx.zsh_plugin_dir)
        .into_iter()
        .map(|p| {
            let h = short(&p, "HEAD").unwrap_or_else(|| "?".into());
            (name(&p), h)
        })
        .collect()
}
