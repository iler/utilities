//! Rust: the rustup toolchains, and the binaries from `cargo install`.
//!
//! Both jobs are in the rust lane. `cargo install` builds with the toolchain, so it must not run
//! while rustup replaces that toolchain.

use super::{Ctx, Item, Job, Need};
use crate::run::shell::{Sh, capture, cmd};
use crate::run::snapshot::Snapshot;
use semver::Version;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

pub fn jobs() -> Vec<Job> {
    let mut rustup = Job::base(
        "rustup",
        "Rust toolchains",
        "rust",
        Need::Bin("rustup"),
        update_rustup,
    );
    rustup.command = "rustup update".into();
    rustup.check_text = "rustup check";
    rustup.check = Some(check_rustup);
    rustup.per_item = true;
    rustup.snapshot = Some(snapshot_rustup);

    let mut cargo = Job::base(
        "cargo",
        "Cargo binaries",
        "rust",
        Need::Bin("cargo"),
        update_cargo,
    );
    cargo.command = "cargo install <name> for each outdated crate".into();
    cargo.check_text = "~/.cargo/.crates2.json and the crates.io index";
    cargo.check = Some(check_cargo);
    cargo.per_item = true;
    cargo.snapshot = Some(snapshot_cargo);

    vec![rustup, cargo]
}

/// Shows the version number only, unless the numbers are equal (a nightly): then the full text.
fn versions(old: &str, new: &str) -> (String, String) {
    let first = |s: &str| s.split_whitespace().next().unwrap_or("").to_string();
    if first(old) != first(new) {
        (first(old), first(new))
    } else {
        (old.to_string(), new.to_string())
    }
}

/// `rustup check` prints:
///
/// ```text
/// stable-aarch64-apple-darwin - Update available : 1.99.0 (abc 2026-10-01) -> 1.100.0 (def 2026-11-05)
/// rustup - Up to date : 1.29.3
/// ```
pub fn parse_check(text: &str) -> Vec<Item> {
    text.lines()
        .filter_map(|l| {
            let (name, rest) = l.trim().split_once(" - ")?;
            let rest = rest.trim().strip_prefix("Update available")?;
            let (_, vers) = rest.split_once(':')?;
            let (old, new) = vers.split_once("->")?;
            let (old, new) = versions(old.trim(), new.trim());
            let item = Item::new(name.trim(), &old, &new);
            Some(if name.trim() == "rustup" {
                item.note("self")
            } else {
                item
            })
        })
        .collect()
}

fn check_rustup(_: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let out = sh.output(cmd("rustup", &["check"]))?;
    if out.stdout.trim().is_empty() && !out.ok() {
        return Err(format!(
            "rustup check failed (exit {})",
            out.code.unwrap_or(-1)
        ));
    }
    Ok(parse_check(&out.stdout))
}

fn update_rustup(_: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    let Some(items) = items else {
        return sh.run(cmd("rustup", &["update"]));
    };
    if items.iter().any(|i| i.name == "rustup") {
        sh.run(cmd("rustup", &["self", "update"]))?;
    }
    let toolchains: Vec<&str> = items
        .iter()
        .filter(|i| i.name != "rustup")
        .map(|i| i.arg())
        .collect();
    if toolchains.is_empty() {
        return Ok(());
    }
    sh.run(cmd("rustup", &["update", "--no-self-update"]).args(toolchains))
}

/// The toolchains with the rustc version of each, and rustup itself.
fn snapshot_rustup(_: &Ctx) -> Snapshot {
    let mut s = Snapshot::new();
    if let Some(v) = capture(&cmd("rustup", &["--version"]))
        && let Some(ver) = v.split_whitespace().nth(1)
    {
        s.insert("rustup".into(), ver.to_string());
    }
    let list = capture(&cmd("rustup", &["toolchain", "list"])).unwrap_or_default();
    for tc in list.lines().filter_map(|l| l.split_whitespace().next()) {
        if tc == "no" {
            continue; // "no installed toolchains"
        }
        let v = capture(&cmd("rustup", &["run", tc, "rustc", "--version"])).unwrap_or_default();
        let v = v.trim().strip_prefix("rustc ").unwrap_or("?").to_string();
        s.insert(tc.to_string(), v);
    }
    s
}

#[derive(Deserialize, Default)]
struct Crates2 {
    #[serde(default)]
    installs: BTreeMap<String, Install>,
}

#[derive(Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Install {
    #[serde(default)]
    pub version_req: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub all_features: bool,
    #[serde(default)]
    pub no_default_features: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Crate {
    pub name: String,
    pub version: String,
    /// From crates.io. Git and path installs are not checked.
    pub registry: bool,
    pub install: Install,
}

/// A key of `.crates2.json`: `ripgrep 14.1.0 (registry+https://github.com/rust-lang/crates.io-index)`.
fn parse_key(key: &str) -> Option<(String, String, String)> {
    let (name, rest) = key.split_once(' ')?;
    let (version, source) = rest.split_once(' ')?;
    let source = source.trim().strip_prefix('(')?.strip_suffix(')')?;
    Some((name.to_string(), version.to_string(), source.to_string()))
}

fn crates_io(source: &str) -> bool {
    source.starts_with("registry+https://github.com/rust-lang/crates.io-index")
        || source.starts_with("sparse+https://index.crates.io/")
}

pub fn parse_crates2(json: &str) -> Vec<Crate> {
    let c: Crates2 = serde_json::from_str(json).unwrap_or_default();
    c.installs
        .into_iter()
        .filter_map(|(k, install)| {
            let (name, version, source) = parse_key(&k)?;
            Some(Crate {
                name,
                version,
                registry: crates_io(&source),
                install,
            })
        })
        .collect()
}

fn installed(ctx: &Ctx) -> Vec<Crate> {
    std::fs::read_to_string(ctx.cargo_home.join(".crates2.json"))
        .map(|t| parse_crates2(&t))
        .unwrap_or_default()
}

/// The path of a crate in the sparse index: `1/a`, `2/ab`, `3/a/abc`, `ri/pg/ripgrep`.
pub fn index_path(name: &str) -> String {
    let n = name.to_lowercase();
    match n.len() {
        1 => format!("1/{n}"),
        2 => format!("2/{n}"),
        3 => format!("3/{}/{n}", &n[..1]),
        _ => format!("{}/{}/{n}", &n[..2], &n[2..4]),
    }
}

#[derive(Deserialize)]
struct IndexLine {
    vers: String,
    #[serde(default)]
    yanked: bool,
}

/// The newest version in an index file that is not yanked. Prereleases count only when the
/// installed version is a prerelease too.
pub fn newest(index: &str, prerelease: bool) -> Option<Version> {
    index
        .lines()
        .filter_map(|l| serde_json::from_str::<IndexLine>(l).ok())
        .filter(|l| !l.yanked)
        .filter_map(|l| Version::parse(&l.vers).ok())
        .filter(|v| prerelease || v.pre.is_empty())
        .max()
}

fn fetch_index(agent: &ureq::Agent, name: &str) -> Result<String, String> {
    let url = format!("https://index.crates.io/{}", index_path(name));
    agent
        .get(&url)
        .call()
        .map_err(|e| format!("{name}: {e}"))?
        .into_string()
        .map_err(|e| format!("{name}: {e}"))
}

fn check_cargo(ctx: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    let mut items = Vec::new();
    let mut errors = Vec::new();
    for c in installed(ctx) {
        if !c.registry {
            sh.say(&format!("Skip {}: it is not from crates.io.", c.name));
            continue;
        }
        if c.install.version_req.is_some() {
            sh.say(&format!(
                "Skip {}: it was installed with --version.",
                c.name
            ));
            continue;
        }
        let Ok(have) = Version::parse(&c.version) else {
            continue;
        };
        match fetch_index(&agent, &c.name) {
            Ok(text) => {
                if let Some(v) = newest(&text, !have.pre.is_empty())
                    && v > have
                {
                    items.push(Item::new(&c.name, &c.version, &v.to_string()));
                }
            }
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        sh.say(&format!(
            "The crates.io index did not answer for: {}",
            errors.join("; ")
        ));
        if items.is_empty() {
            return Err("cannot read the crates.io index".into());
        }
    }
    Ok(items)
}

/// The `cargo install` arguments that keep the features of the first install.
pub fn install_args(c: &Crate) -> Vec<String> {
    let mut a = vec!["install".to_string(), c.name.clone()];
    if c.install.all_features {
        a.push("--all-features".into());
    } else if !c.install.features.is_empty() {
        a.push("--features".into());
        a.push(c.install.features.join(","));
    }
    if c.install.no_default_features {
        a.push("--no-default-features".into());
    }
    a
}

/// A failed crate does not stop the other crates, but the job fails at the end.
fn update_cargo(ctx: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    let list = match items {
        Some(items) => items.to_vec(),
        None => check_cargo(ctx, sh)?,
    };
    if list.is_empty() {
        sh.say("All cargo binaries are up to date.");
        return Ok(());
    }
    let crates = installed(ctx);
    let mut failed = Vec::new();
    for item in &list {
        let c = crates
            .iter()
            .find(|c| c.name == item.name)
            .cloned()
            .unwrap_or(Crate {
                name: item.name.clone(),
                version: item.current.clone(),
                registry: true,
                install: Install::default(),
            });
        if sh.run(cmd("cargo", &[]).args(install_args(&c))).is_err() {
            if sh.stopped() {
                return Err("stopped".into());
            }
            failed.push(item.name.clone());
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("cargo install failed for: {}", failed.join(", ")))
    }
}

fn snapshot_cargo(ctx: &Ctx) -> Snapshot {
    installed(ctx)
        .into_iter()
        .map(|c| (c.name, c.version))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rustup_check() {
        let items = parse_check(include_str!("../../tests/fixtures/rustup_check.txt"));
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0],
            Item::new("stable-aarch64-apple-darwin", "1.99.0", "1.100.0")
        );
        assert_eq!(items[1].current, "1.101.0-nightly (aaa1111 2026-11-01)");
        assert_eq!(
            items[2],
            Item::new("rustup", "1.29.3", "1.30.0").note("self")
        );
    }

    #[test]
    fn reads_crates2() {
        let crates = parse_crates2(include_str!("../../tests/fixtures/crates2.json"));
        assert_eq!(crates.len(), 3);
        let rg = crates.iter().find(|c| c.name == "ripgrep").unwrap();
        assert!(rg.registry);
        assert_eq!(rg.version, "14.1.0");
        assert_eq!(
            install_args(rg),
            vec![
                "install",
                "ripgrep",
                "--features",
                "pcre2",
                "--no-default-features"
            ]
        );
        assert!(!crates.iter().find(|c| c.name == "mytool").unwrap().registry);
        assert!(crates.iter().find(|c| c.name == "bat").unwrap().registry);
    }

    #[test]
    fn index_paths_follow_the_cargo_rules() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("RipGrep"), "ri/pg/ripgrep");
    }

    #[test]
    fn newest_skips_yanked_and_prereleases() {
        let index = include_str!("../../tests/fixtures/crates_index_ripgrep.txt");
        assert_eq!(newest(index, false), Some(Version::new(14, 1, 1)));
        assert_eq!(
            newest(index, true).map(|v| v.to_string()),
            Some("15.0.0-beta.1".into())
        );
    }
}
