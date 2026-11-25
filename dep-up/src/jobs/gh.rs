//! GitHub CLI extensions.

use super::{Ctx, Item, Job, Need, note_last_line};
use crate::run::shell::{Sh, capture, cmd};
use crate::run::snapshot::Snapshot;

pub fn job() -> Job {
    let mut j = Job::base("gh", "gh extensions", "gh", Need::Bin("gh"), update);
    j.command = "gh extension upgrade --all".into();
    j.check_text = "gh extension upgrade --all --dry-run";
    j.check = Some(check);
    j.per_item = true;
    j.snapshot = Some(snapshot);
    j.note = Some(note_last_line);
    j
}

/// The dry run prints `[  name]: would have upgraded from v1.0.0 to v1.1.0` for each extension
/// that can change, and `[name]: already up to date` for the others.
pub fn parse_dry_run(text: &str) -> Vec<Item> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix('[')?;
            let (name, rest) = l.split_once("]:")?;
            let rest = rest.trim().strip_prefix("would have upgraded from ")?;
            let (old, new) = rest.split_once(" to ")?;
            Some(Item::new(name.trim(), old.trim(), new.trim()))
        })
        .collect()
}

fn check(_: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let out = sh.output(cmd("gh", &["extension", "upgrade", "--all", "--dry-run"]))?;
    let items = parse_dry_run(&out.stdout);
    if !out.ok() && items.is_empty() {
        let text = format!("{}{}", out.stdout, out.stderr).to_lowercase();
        // Without extensions, gh reports an error. That is not a failed check.
        if text.contains("no installed extensions") || text.contains("no extensions") {
            return Ok(Vec::new());
        }
        return Err(format!(
            "gh extension upgrade failed (exit {})",
            out.code.unwrap_or(-1)
        ));
    }
    Ok(items)
}

fn update(_: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    match items {
        // Without extensions, gh fails with "no extensions installed".
        None if snapshot(&Ctx::from_env()).is_empty() => {
            sh.say("No gh extensions are installed.");
            Ok(())
        }
        None => sh.run(cmd("gh", &["extension", "upgrade", "--all"])),
        // gh upgrades one named extension at a time.
        Some(items) => items
            .iter()
            .try_for_each(|i| sh.run(cmd("gh", &["extension", "upgrade", i.arg()]))),
    }
}

/// `gh extension list` without a terminal prints `gh <name>\t<repo>\t<version>`.
pub fn parse_list(text: &str) -> Snapshot {
    text.lines()
        .filter_map(|l| {
            let mut cols = l.split('\t');
            let name = cols.next()?.trim();
            let name = name.strip_prefix("gh ").unwrap_or(name);
            let _repo = cols.next()?;
            let version = cols.next().unwrap_or("?").trim();
            (!name.is_empty()).then(|| (name.to_string(), version.to_string()))
        })
        .collect()
}

fn snapshot(_: &Ctx) -> Snapshot {
    capture(&cmd("gh", &["extension", "list"]))
        .map(|o| parse_list(&o))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_dry_run() {
        let text = "[   dash]: would have upgraded from v4.0.0 to v4.1.0\n[copilot]: already up to date\n[x]: some error\n";
        assert_eq!(
            parse_dry_run(text),
            vec![Item::new("dash", "v4.0.0", "v4.1.0")]
        );
    }

    #[test]
    fn reads_the_list() {
        let s =
            parse_list("gh dash\tdlvhdr/gh-dash\tv4.0.0\ngh copilot\tgithub/gh-copilot\tv1.0.5\n");
        assert_eq!(s.get("dash").map(String::as_str), Some("v4.0.0"));
        assert_eq!(s.len(), 2);
    }
}
