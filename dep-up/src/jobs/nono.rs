//! nono packs. The job runs in the brew lane, after `brew upgrade`: Homebrew installs the
//! `nono` program, a new pack can need the new program, and the program must not change while
//! the packs update. `nono update` skips pinned packs.

use super::{Ctx, Item, Job, Need};
use crate::run::shell::{Sh, capture, cmd};
use crate::run::snapshot::Snapshot;
use serde::Deserialize;
use std::collections::BTreeMap;

pub fn job() -> Job {
    let mut j = Job::base("nono", "nono packs", "brew", Need::Bin("nono"), update);
    j.command = "nono update".into();
    j.check_text = "nono outdated --json";
    j.check = Some(check);
    j.per_item = true;
    j.snapshot = Some(snapshot);
    j
}

#[derive(Deserialize)]
struct Outdated {
    key: String,
    #[serde(default)]
    installed: String,
    #[serde(default)]
    latest: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    pinned: bool,
}

pub fn parse_outdated(json: &str) -> Result<Vec<Item>, String> {
    let list: Vec<Outdated> =
        serde_json::from_str(json).map_err(|e| format!("cannot read nono outdated: {e}"))?;
    Ok(list
        .into_iter()
        .filter(|p| p.status != "current" && p.installed != p.latest)
        .map(|p| {
            Item::new(&p.key, &p.installed, &p.latest).note(if p.pinned { "pinned" } else { "" })
        })
        .collect())
}

fn check(_: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let out = sh.output(cmd("nono", &["outdated", "--json"]))?;
    let text = out.stdout.trim();
    if text.is_empty() && out.ok() {
        return Ok(Vec::new());
    }
    parse_outdated(text)
}

fn update(_: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    match items {
        None => sh.run(cmd("nono", &["update"])),
        // nono update takes one pack at a time.
        Some(items) => items
            .iter()
            .try_for_each(|i| sh.run(cmd("nono", &["update", i.arg()]))),
    }
}

#[derive(Deserialize)]
struct Installed {
    #[serde(default)]
    packages: BTreeMap<String, Pack>,
}

#[derive(Deserialize)]
struct Pack {
    #[serde(default)]
    version: Option<String>,
}

pub fn parse_installed(json: &str) -> Snapshot {
    serde_json::from_str::<Installed>(json)
        .map(|i| {
            i.packages
                .into_iter()
                .map(|(k, p)| (k, p.version.unwrap_or_else(|| "?".into())))
                .collect()
        })
        .unwrap_or_default()
}

fn snapshot(_: &Ctx) -> Snapshot {
    capture(&cmd("nono", &["list", "--installed", "--json"]))
        .map(|o| parse_installed(&o))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_outdated_packs() {
        let items =
            parse_outdated(include_str!("../../tests/fixtures/nono_outdated.json")).unwrap();
        assert_eq!(
            items,
            vec![Item::new("nolabs-ai/codex", "0.1.0", "0.2.0").note("pinned")]
        );
    }

    #[test]
    fn reads_installed_packs() {
        let s = parse_installed(
            r#"{"lockfile_version":4,"packages":{"nolabs-ai/claude":{"version":"0.1.5","pinned":false}}}"#,
        );
        assert_eq!(s.get("nolabs-ai/claude").map(String::as_str), Some("0.1.5"));
    }
}
