//! Homebrew: the index, the formulae and casks, the Brewfile backup and the cleanup.

use super::{Ctx, Item, Job, Kind, Need, note_last_line};
use crate::run::shell::{Sh, capture, cmd, in_path};
use crate::run::snapshot::Snapshot;
use serde::Deserialize;

pub fn jobs(ctx: &Ctx) -> Vec<Job> {
    let mut index = Job::base(
        "brew-index",
        "Homebrew index",
        "brew",
        Need::Bin("brew"),
        update_index,
    );
    index.prep = true;
    index.command = "brew update".into();
    index.note = Some(note_last_line);

    let mut apps = Job::base(
        "brew-apps",
        "Homebrew apps",
        "brew",
        Need::Bin("brew"),
        update_apps,
    );
    apps.command = "brew upgrade".into();
    apps.check_text = "brew outdated --json=v2";
    apps.check = Some(check_apps);
    apps.per_item = true;
    apps.snapshot = Some(snapshot);

    let mut backup = Job::base(
        "brew-backup",
        "Homebrew backup",
        "brew",
        Need::Bin("brew"),
        update_backup,
    );
    backup.kind = Kind::Backup;
    backup.command = format!(
        "brew bundle dump --force --file={}",
        crate::run::logs::pretty(&ctx.brewfile())
    );
    backup.note = Some(note_last_line);

    let mut cleanup = Job::base(
        "brew-cleanup",
        "Homebrew cleanup",
        "brew",
        Need::Bin("brew"),
        update_cleanup,
    );
    cleanup.command = "brew cleanup --prune=all".into();
    cleanup.note = Some(note_cleanup);

    let nono = super::nono::job();
    vec![index, apps, nono, backup, cleanup]
}

fn update_index(_: &Ctx, sh: &Sh, _: Option<&[Item]>) -> Result<(), String> {
    sh.run(cmd("brew", &["update"]))
}

fn check_apps(_: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let out = sh.output(cmd("brew", &["outdated", "--json=v2"]))?;
    match parse_outdated(&out.stdout) {
        Ok(items) => Ok(items),
        Err(e) if out.ok() => Err(e),
        Err(_) => Err(format!(
            "brew outdated failed (exit {})",
            out.code.unwrap_or(-1)
        )),
    }
}

#[derive(Deserialize)]
struct Outdated {
    #[serde(default)]
    formulae: Vec<Entry>,
    #[serde(default)]
    casks: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    #[serde(default)]
    installed_versions: Vec<String>,
    #[serde(default)]
    current_version: String,
    #[serde(default)]
    pinned: bool,
}

pub fn parse_outdated(json: &str) -> Result<Vec<Item>, String> {
    let o: Outdated =
        serde_json::from_str(json).map_err(|e| format!("cannot read brew outdated: {e}"))?;
    let item = |e: &Entry, cask: bool| {
        let current = e.installed_versions.last().cloned().unwrap_or_default();
        let note = match (cask, e.pinned) {
            (true, _) => "cask",
            (false, true) => "pinned",
            _ => "",
        };
        Item::new(&e.name, &current, &e.current_version).note(note)
    };
    let mut items: Vec<Item> = o.formulae.iter().map(|e| item(e, false)).collect();
    items.extend(o.casks.iter().map(|e| item(e, true)));
    Ok(items)
}

fn update_apps(_: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    let Some(items) = items else {
        return sh.run(cmd("brew", &["upgrade"]));
    };
    // A cask and a formula can have the same name. Tell brew which one.
    let (casks, formulae): (Vec<&Item>, Vec<&Item>) = items.iter().partition(|i| i.note == "cask");
    if !formulae.is_empty() {
        sh.run(cmd("brew", &["upgrade", "--formula"]).args(formulae.iter().map(|i| i.arg())))?;
    }
    if !casks.is_empty() {
        sh.run(cmd("brew", &["upgrade", "--cask"]).args(casks.iter().map(|i| i.arg())))?;
    }
    Ok(())
}

/// `brew list --versions` prints `<name> <version> [<version> ...]`.
pub fn parse_list(text: &str) -> Snapshot {
    text.lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            let name = words.next()?;
            let versions: Vec<&str> = words.collect();
            Some((name.to_string(), versions.join(" ")))
        })
        .collect()
}

fn snapshot(_: &Ctx) -> Snapshot {
    let mut s = Snapshot::new();
    for kind in ["--formula", "--cask"] {
        if let Some(out) = capture(&cmd("brew", &["list", "--versions", kind])) {
            s.extend(parse_list(&out));
        }
    }
    s
}

/// Writes a Brewfile with the taps, the formulae, the casks and the App Store apps.
fn update_backup(ctx: &Ctx, sh: &Sh, _: Option<&[Item]>) -> Result<(), String> {
    let file = ctx.brewfile();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let mut c = cmd("brew", &["bundle", "dump", "--force"])
        .arg(format!("--file={}", file.display()))
        .args(["--formula", "--cask", "--tap"]);
    if in_path("mas") {
        c = c.arg("--mas");
    }
    sh.run(c)
}

fn update_cleanup(_: &Ctx, sh: &Sh, _: Option<&[Item]>) -> Result<(), String> {
    sh.run(cmd("brew", &["cleanup", "--prune=all"]))
}

/// Homebrew prints: `==> This operation has freed approximately 599.7MB of disk space.`
pub fn note_cleanup(text: &str) -> Option<String> {
    text.lines().rev().find_map(|l| {
        let rest = l.split("freed approximately ").nth(1)?;
        let size = rest.split(" of disk space").next()?;
        Some(format!("{} freed", size.trim()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_outdated_formulae_and_casks() {
        let items =
            parse_outdated(include_str!("../../tests/fixtures/brew_outdated.json")).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], Item::new("git", "2.43.0", "2.44.0"));
        assert_eq!(items[1].note, "pinned");
        assert_eq!(items[2], Item::new("iterm2", "3.5.0", "3.5.1").note("cask"));
    }

    #[test]
    fn reads_the_installed_versions() {
        let s = parse_list("git 2.44.0\njq 1.7 1.7.1\n\n");
        assert_eq!(s.get("jq").map(String::as_str), Some("1.7 1.7.1"));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn finds_the_freed_space() {
        let out =
            "Removing: x\n==> This operation has freed approximately 599.7MB of disk space.\n";
        assert_eq!(note_cleanup(out), Some("599.7MB freed".into()));
        assert_eq!(note_cleanup("nothing\n"), None);
    }
}
