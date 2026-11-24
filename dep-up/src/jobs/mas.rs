//! App Store apps through `mas`. This is a lane of its own: a failed `softwareupdate` must not
//! stop the App Store updates.

use super::{Ctx, Item, Job, Need};
use crate::run::shell::{Sh, capture, cmd};
use crate::run::snapshot::Snapshot;

pub fn job() -> Job {
    let mut j = Job::base("mas", "App Store apps", "mas", Need::Bin("mas"), update);
    j.command = "mas upgrade".into();
    j.check_text = "mas outdated";
    j.check = Some(check);
    j.per_item = true;
    j.snapshot = Some(snapshot);
    j
}

/// Splits `<id> <name> (<version>)` into its parts. The name can hold spaces.
fn split(line: &str) -> Option<(&str, &str, &str)> {
    let t = line.trim();
    let (id, rest) = t.split_once(char::is_whitespace)?;
    if !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let rest = rest.trim();
    let open = rest.rfind('(')?;
    let inner = rest[open + 1..].strip_suffix(')')?;
    Some((id, rest[..open].trim(), inner.trim()))
}

/// `mas outdated`: `497799835 Xcode (15.0 -> 15.1)`.
pub fn parse_outdated(text: &str) -> Vec<Item> {
    text.lines()
        .filter_map(split)
        .map(|(id, name, versions)| {
            let (old, new) = versions.split_once("->").unwrap_or(("", versions));
            Item::new(name, old.trim(), new.trim()).id(id)
        })
        .collect()
}

/// `mas list`: `1091189122  Bear  (3.0.6)`.
pub fn parse_list(text: &str) -> Snapshot {
    text.lines()
        .filter_map(split)
        .map(|(_, name, ver)| (name.to_string(), ver.to_string()))
        .collect()
}

fn check(_: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let out = sh.output(cmd("mas", &["outdated"]))?;
    if !out.ok() {
        return Err(format!(
            "mas outdated failed (exit {})",
            out.code.unwrap_or(-1)
        ));
    }
    Ok(parse_outdated(&out.stdout))
}

fn update(_: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    let c = cmd("mas", &["upgrade"]);
    sh.run(match items {
        None => c,
        Some(items) => c.args(super::args(items)),
    })
}

fn snapshot(_: &Ctx) -> Snapshot {
    capture(&cmd("mas", &["list"]))
        .map(|o| parse_list(&o))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_outdated_apps() {
        let items = parse_outdated(
            "497799835 Xcode (15.0 -> 15.1)\n 409183694  Final Cut Pro  (10.7 -> 10.8)\n",
        );
        assert_eq!(items[0], Item::new("Xcode", "15.0", "15.1").id("497799835"));
        assert_eq!(items[1].name, "Final Cut Pro");
        assert_eq!(items[1].arg(), "409183694");
    }

    #[test]
    fn reads_installed_apps() {
        let s = parse_list(
            "1091189122  Bear            (3.0.6)\n 412529613  Cinch           (1.2.4)\nbad line\n",
        );
        assert_eq!(s.get("Bear").map(String::as_str), Some("3.0.6"));
        assert_eq!(s.len(), 2);
    }
}
