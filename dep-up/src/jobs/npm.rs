//! The node lane: Claude skills, the npm globals, UI/UX Pro Max and the npm backup.

use super::{Ctx, Item, Job, Kind, Need, note_last_line};
use crate::run::shell::{Sh, capture, cmd};
use crate::run::snapshot::Snapshot;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub fn jobs(ctx: &Ctx) -> Vec<Job> {
    let mut skills = Job::base(
        "skills",
        "Claude skills",
        "node",
        Need::Bin("npx"),
        update_skills,
    );
    // `-g -y` stops the scope question and always updates the global skills. This also
    // matches the lock file that the change list reads.
    skills.command = "npx --yes skills update -g -y".into();
    skills.snapshot = Some(snapshot_skills);

    let mut globals = Job::base(
        "npm-globals",
        "npm globals",
        "node",
        Need::Bin("npm"),
        update_globals,
    );
    globals.command = "npm update -g".into();
    globals.check_text = "npm outdated -g --json";
    globals.check = Some(check_globals);
    globals.per_item = true;
    globals.snapshot = Some(snapshot_globals);

    let mut uipro = Job::base(
        "uipro",
        "UI/UX Pro Max",
        "node",
        Need::Bin("uipro"),
        update_uipro,
    );
    uipro.command = format!(
        "uipro update --global --ai <type>, for {}",
        ctx.uipro_ai.join(", ")
    );
    uipro.snapshot = Some(snapshot_uipro);

    let mut backup = Job::base(
        "npm-backup",
        "npm backup",
        "node",
        Need::Bin("npm"),
        update_backup,
    );
    backup.kind = Kind::Backup;
    backup.command = format!(
        "npm list --global --depth=0 --json > {}",
        crate::run::logs::pretty(&ctx.npm_list())
    );
    backup.note = Some(note_last_line);

    vec![skills, globals, uipro, backup]
}

fn update_skills(_: &Ctx, sh: &Sh, _: Option<&[Item]>) -> Result<(), String> {
    sh.run(cmd("npx", &["--yes", "skills", "update", "-g", "-y"]))
}

#[derive(Deserialize)]
struct SkillLock {
    #[serde(default)]
    skills: BTreeMap<String, Skill>,
}

#[derive(Deserialize)]
struct Skill {
    #[serde(default, rename = "skillFolderHash")]
    hash: Option<String>,
}

/// Skills have no version, only a content hash.
pub fn parse_skill_lock(json: &str) -> Snapshot {
    serde_json::from_str::<SkillLock>(json)
        .map(|l| {
            l.skills
                .into_iter()
                .map(|(k, s)| {
                    let h = s.hash.unwrap_or_default();
                    (
                        k,
                        if h.is_empty() {
                            "?".into()
                        } else {
                            h.chars().take(7).collect()
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn snapshot_skills(ctx: &Ctx) -> Snapshot {
    std::fs::read_to_string(&ctx.skill_lock)
        .map(|t| parse_skill_lock(&t))
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct Outdated {
    #[serde(default)]
    current: Option<String>,
    #[serde(default)]
    latest: Option<String>,
}

pub fn parse_outdated(json: &str) -> Result<Vec<Item>, String> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let map: BTreeMap<String, Outdated> =
        serde_json::from_str(json).map_err(|e| format!("cannot read npm outdated: {e}"))?;
    Ok(map
        .into_iter()
        .filter_map(|(name, o)| {
            let (current, latest) = (o.current?, o.latest?);
            (current != latest).then(|| Item::new(&name, &current, &latest))
        })
        .collect())
}

fn check_globals(_: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    // npm outdated exits with status 1 when something is outdated.
    let out = sh.output(cmd("npm", &["outdated", "-g", "--json"]))?;
    parse_outdated(&out.stdout)
}

fn update_globals(_: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    match items {
        None => sh.run(cmd("npm", &["update", "-g"])),
        Some(items) => sh.run(
            cmd("npm", &["install", "-g"])
                .args(items.iter().map(|i| format!("{}@latest", i.arg()))),
        ),
    }
}

/// `npm ls -g --depth=0 --json`: name → version.
pub fn parse_ls(json: &str) -> Snapshot {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Snapshot::new();
    };
    v.get("dependencies")
        .and_then(Value::as_object)
        .map(|deps| {
            deps.iter()
                .map(|(k, d)| {
                    let ver = d.get("version").and_then(Value::as_str).unwrap_or("?");
                    (k.clone(), ver.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn snapshot_globals(_: &Ctx) -> Snapshot {
    capture(&cmd("npm", &["ls", "-g", "--depth=0", "--json"]))
        .map(|o| parse_ls(&o))
        .unwrap_or_default()
}

fn uipro_version() -> String {
    capture(&cmd("uipro", &["--version"]))
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

/// Without `--ai`, uipro asks for the assistant, and a job gets no keyboard input. So the job
/// runs one update for each type. When the CLI itself changes, the update does not refresh the
/// skill files, so the job then runs it again with the new CLI.
fn update_uipro(ctx: &Ctx, sh: &Sh, _: Option<&[Item]>) -> Result<(), String> {
    for ai in &ctx.uipro_ai {
        let before = uipro_version();
        sh.run(cmd("uipro", &["update", "--global", "--ai", ai]))?;
        if uipro_version() != before {
            sh.say("The uipro CLI changed. Run the update again with the new CLI.");
            sh.run(cmd("uipro", &["update", "--global", "--ai", ai]))?;
        }
    }
    Ok(())
}

/// uipro makes the skill files from its own package, so the CLI version is the skill version.
fn snapshot_uipro(_: &Ctx) -> Snapshot {
    let v = uipro_version();
    if v.is_empty() {
        Snapshot::new()
    } else {
        Snapshot::from([("ui-ux-pro-max".to_string(), v)])
    }
}

/// npm list reports a bad dependency tree with exit status 1, but it still prints the list.
/// Accept that when the list is valid JSON.
fn update_backup(ctx: &Ctx, sh: &Sh, _: Option<&[Item]>) -> Result<(), String> {
    let file = ctx.npm_list();
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let out = sh.output(cmd("npm", &["list", "--global", "--depth=0", "--json"]))?;
    if serde_json::from_str::<Value>(&out.stdout).is_err() {
        return Err("npm list printed no valid JSON".into());
    }
    std::fs::write(&file, &out.stdout)
        .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    if !out.ok() {
        sh.say("npm reported a problem in the global tree. The list is written all the same.");
    }
    sh.say(&format!("Wrote {}", crate::run::logs::pretty(&file)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_npm_outdated() {
        let items = parse_outdated(include_str!("../../tests/fixtures/npm_outdated.json")).unwrap();
        assert_eq!(
            items,
            vec![
                Item::new("githits", "0.11.4", "0.12.0"),
                Item::new("pnpm", "9.0.0", "9.1.0")
            ]
        );
        assert_eq!(parse_outdated("").unwrap(), vec![]);
    }

    #[test]
    fn reads_npm_ls() {
        let s = parse_ls(include_str!("../../tests/fixtures/npm_ls.json"));
        assert_eq!(s.get("githits").map(String::as_str), Some("0.12.0"));
        assert_eq!(s.get("local-tool").map(String::as_str), Some("1.0.0"));
    }

    #[test]
    fn reads_the_skill_lock() {
        let s =
            parse_skill_lock(r#"{"skills":{"tdd":{"skillFolderHash":"0123456789abcdef"},"x":{}}}"#);
        assert_eq!(s.get("tdd").map(String::as_str), Some("0123456"));
        assert_eq!(s.get("x").map(String::as_str), Some("?"));
    }
}
