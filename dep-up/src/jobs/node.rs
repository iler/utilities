//! Node through nvm: the newest LTS with the newest npm, and the global packages it misses.
//!
//! The job runs last in the node lane. `nvm install --latest-npm` can write npm into the node
//! that you use now, which must not happen while `npm update -g` runs. A failed download of a
//! new node then also loses nothing.

use super::{Ctx, Item, Job, Need};
use crate::run::shell::{Sh, cmd};
use crate::run::snapshot::Snapshot;
use semver::Version;
use serde_json::Value;
use std::collections::BTreeMap;

pub fn job(ctx: &Ctx) -> Job {
    let mut j = Job::base(
        "node",
        "Node (nvm)",
        "node",
        Need::File(ctx.nvm_dir.join("nvm.sh")),
        update,
    );
    j.command =
        "nvm install --lts --latest-npm, then npm install -g for missing global packages".into();
    j.check_text = "nvm version-remote --lts";
    j.check = Some(check);
    j.snapshot = Some(snapshot);
    j
}

/// nvm.sh reads unset variables, so it needs `set +u`. The script runs in its own bash.
const CHECK: &str = r#"set +u
. "$NVM_DIR/nvm.sh" >/dev/null 2>&1 || exit 1
printf 'current=%s\n' "$(nvm current)"
printf 'target=%s\n' "$(nvm version-remote --lts)"
"#;

/// nvm fails when the source node is the same as the new node, so the copy option is used only
/// when the newest LTS is not installed yet.
const UPDATE: &str = r#"set +u
. "$NVM_DIR/nvm.sh" || exit 1
from=$(nvm current)
target=$(nvm version-remote --lts)
case "$from" in v*) ;; *) from="" ;; esac
case "$target" in v*) ;; *) target="" ;; esac
if [ -n "$from" ] && [ -n "$target" ] && [ "$(nvm version "$target")" = "N/A" ]; then
  nvm install --lts --latest-npm --reinstall-packages-from="$from" || exit 1
else
  nvm install --lts --latest-npm || exit 1
fi
target=$(nvm version "${target:-lts/*}") || exit 1
case "$target" in v*) ;; *) exit 1 ;; esac
printf 'from=%s\ntarget=%s\n' "$from" "$target" > "$DEP_UP_OUT"
"#;

fn keys(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

fn installed(ctx: &Ctx, version: &str) -> bool {
    ctx.nvm_dir.join("versions/node").join(version).is_dir()
}

fn check(ctx: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let out =
        sh.output(cmd("bash", &["-c", CHECK]).env("NVM_DIR", ctx.nvm_dir.display().to_string()))?;
    let k = keys(&out.stdout);
    let target = k.get("target").cloned().unwrap_or_default();
    if !target.starts_with('v') {
        return Err("nvm did not find the newest LTS version".into());
    }
    if installed(ctx, &target) {
        return Ok(Vec::new());
    }
    let current = k
        .get("current")
        .filter(|c| c.starts_with('v'))
        .cloned()
        .unwrap_or_default();
    Ok(vec![Item::new("node", &current, &target).note("LTS")])
}

fn update(ctx: &Ctx, sh: &Sh, _: Option<&[Item]>) -> Result<(), String> {
    let out_file = std::env::temp_dir().join(format!("dep-up-node-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&out_file);
    sh.run(
        cmd("bash", &["-c", UPDATE])
            .env("NVM_DIR", ctx.nvm_dir.display().to_string())
            .env("DEP_UP_OUT", out_file.display().to_string()),
    )?;
    let k = keys(&std::fs::read_to_string(&out_file).unwrap_or_default());
    let _ = std::fs::remove_file(&out_file);
    let target = k.get("target").cloned().unwrap_or_default();
    if !target.starts_with('v') {
        return Err("nvm did not report the installed version".into());
    }
    let from = k.get("from").cloned().unwrap_or_default();
    sync_globals(ctx, sh, &target, &from)
}

/// The installed node versions (`vX.Y.Z` only), oldest first.
fn versions(ctx: &Ctx) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(ctx.nvm_dir.join("versions/node")) else {
        return Vec::new();
    };
    let names: Vec<String> = dir
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect();
    sort_versions(names)
}

pub fn sort_versions(names: Vec<String>) -> Vec<String> {
    let mut v: Vec<(Version, String)> = names
        .into_iter()
        .filter_map(|n| {
            let ver = Version::parse(n.strip_prefix('v')?).ok()?;
            ver.pre.is_empty().then_some((ver, n))
        })
        .collect();
    v.sort();
    v.into_iter().map(|(_, n)| n).collect()
}

/// The newest installed version that is older than `target`.
pub fn previous_of(versions: &[String], target: &str) -> Option<String> {
    let i = versions.iter().position(|v| v == target)?;
    i.checked_sub(1).map(|p| versions[p].clone())
}

/// The global packages of a node, from the npm of that node. npm can report a bad tree and
/// still print a valid list: accept the list, but fail without usable JSON.
fn packages(ctx: &Ctx, sh: &Sh, version: &str) -> Result<Value, String> {
    let bin = ctx.nvm_dir.join("versions/node").join(version).join("bin");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = sh.output(cmd("npm", &["ls", "-g", "--depth=0", "--json"]).env("PATH", path))?;
    serde_json::from_str(&out.stdout)
        .map_err(|_| format!("cannot read the global packages of node {version}"))
}

pub fn names(packages: &Value) -> Vec<String> {
    packages
        .get("dependencies")
        .and_then(Value::as_object)
        .map(|d| d.keys().cloned().collect())
        .unwrap_or_default()
}

/// The packages to copy: not npm or corepack (each node has its own), and not linked local
/// packages.
pub fn copyable(packages: &Value) -> Vec<String> {
    let local = |v: Option<&Value>| {
        v.and_then(Value::as_str)
            .is_some_and(|s| s.starts_with("file:") || s.starts_with("link:"))
    };
    packages
        .get("dependencies")
        .and_then(Value::as_object)
        .map(|d| {
            d.iter()
                .filter(|(k, _)| *k != "npm" && *k != "corepack")
                .filter(|(_, v)| v.get("link").and_then(Value::as_bool) != Some(true))
                .filter(|(_, v)| !local(v.get("resolved")) && !local(v.get("version")))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Copies missing global packages from the node that was in use before, and from the newest
/// installed node that is older than the target. It installs by name, so npm gets the latest
/// versions.
fn sync_globals(ctx: &Ctx, sh: &Sh, target: &str, from: &str) -> Result<(), String> {
    let all = versions(ctx);
    let previous = previous_of(&all, target)
        .filter(|p| p != from)
        .unwrap_or_default();
    let mut have = names(&packages(ctx, sh, target)?);
    let mut missing = Vec::new();
    for source in [from, previous.as_str()] {
        if source.is_empty() || source == target {
            continue;
        }
        for p in copyable(&packages(ctx, sh, source)?) {
            if !have.contains(&p) {
                have.push(p.clone());
                missing.push(p);
            }
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    sh.say(&format!(
        "Install missing global packages: {}",
        missing.join(" ")
    ));
    let bin = ctx.nvm_dir.join("versions/node").join(target).join("bin");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    sh.run(
        cmd("npm", &["install", "-g"])
            .args(missing)
            .env("PATH", path),
    )
}

/// One line for each installed node. The version column holds the npm inside that node.
fn snapshot(ctx: &Ctx) -> Snapshot {
    versions(ctx)
        .into_iter()
        .map(|v| {
            let pkg = ctx
                .nvm_dir
                .join("versions/node")
                .join(&v)
                .join("lib/node_modules/npm/package.json");
            let npm = std::fs::read_to_string(pkg)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .and_then(|j| j.get("version").and_then(Value::as_str).map(String::from))
                .unwrap_or_else(|| "?".into());
            (v, format!("npm {npm}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_versions_by_number() {
        let v = sort_versions(vec![
            "v20.10.0".into(),
            "v9.0.0".into(),
            "v20.9.0".into(),
            "system".into(),
        ]);
        assert_eq!(v, vec!["v9.0.0", "v20.9.0", "v20.10.0"]);
        assert_eq!(previous_of(&v, "v20.10.0"), Some("v20.9.0".into()));
        assert_eq!(previous_of(&v, "v9.0.0"), None);
    }

    #[test]
    fn copies_only_registry_packages() {
        let p: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/npm_ls.json")).unwrap();
        assert_eq!(copyable(&p), vec!["githits", "pnpm"]);
        assert_eq!(names(&p).len(), 5);
    }
}
