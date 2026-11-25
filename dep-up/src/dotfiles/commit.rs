//! The commit of the package lists (key `c`).
//!
//! The commit holds two files only: the Brewfile and the npm package list. With chezmoi, these
//! are the copies in the chezmoi source directory. Without chezmoi, they are the backup files,
//! if they are in a git repository. Other staged or changed files stay as they are.
//!
//! The title is a conventional commit of what changed since the last commit, made less
//! specific until it fits in 72 characters. The body lists every change, unless the title
//! already names each change with its versions.

use crate::jobs::Ctx;
use crate::run::snapshot::{Mark, Snapshot, diff};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const TITLE_MAX: usize = 72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    Brewfile,
    Npm,
}

impl List {
    pub fn label(self) -> &'static str {
        match self {
            List::Brewfile => "Brewfile",
            List::Npm => "npm globals",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub list: List,
    pub mark: Mark,
    /// For the Brewfile: `brew`, `cask`, `tap`, `mas`, ...
    pub kind: String,
    pub name: String,
    pub old: String,
    pub new: String,
}

impl Entry {
    fn verb(&self) -> &'static str {
        match self.mark {
            Mark::Up => "upgrade",
            Mark::Add => "add",
            Mark::Remove => "remove",
        }
    }
    /// The name with its kind: `jq`, `cask iterm2`, `tap homebrew/core`.
    fn subject(&self) -> String {
        match self.kind.as_str() {
            "" | "brew" => self.name.clone(),
            "mas" => format!("app {}", self.name),
            "vscode" => format!("VS Code extension {}", self.name),
            k => format!("{k} {}", self.name),
        }
    }
    /// `upgrade githits 0.11.4 to 0.12.0`, `add jq`, `add pnpm 9.1.0`, `remove cask iterm2`.
    pub fn phrase(&self) -> String {
        match self.mark {
            Mark::Up => format!("upgrade {} {} to {}", self.subject(), self.old, self.new),
            Mark::Add if !self.new.is_empty() => format!("add {} {}", self.subject(), self.new),
            Mark::Add => format!("add {}", self.subject()),
            Mark::Remove => format!("remove {}", self.subject()),
        }
    }
}

/// The repository and the two files in it.
#[derive(Clone, Debug)]
pub struct Target {
    pub repo: PathBuf,
    /// Paths relative to the repository.
    pub files: Vec<(PathBuf, List)>,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub repo: PathBuf,
    /// The files that changed since the last commit, relative to the repository.
    pub files: Vec<PathBuf>,
    pub entries: Vec<Entry>,
}

fn git(repo: &Path, args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
}

fn toplevel(dir: &Path) -> Option<PathBuf> {
    let out = git(dir, &["rev-parse", "--show-toplevel"]).ok()?;
    out.status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

/// The full path, also for a file that does not exist yet (its directory must exist).
fn real(p: &Path) -> Option<PathBuf> {
    if let Ok(r) = p.canonicalize() {
        return Some(r);
    }
    Some(p.parent()?.canonicalize().ok()?.join(p.file_name()?))
}

fn chezmoi_path(target: &Path) -> Option<PathBuf> {
    let out = Command::new("chezmoi")
        .arg("source-path")
        .arg(target)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

/// Finds the repository of the package lists.
pub fn target(ctx: &Ctx) -> Result<Target, String> {
    let lists = [
        (ctx.brewfile(), List::Brewfile),
        (ctx.npm_list(), List::Npm),
    ];
    let paths: Vec<(PathBuf, List)> = if super::chezmoi_source().is_some() {
        let p: Vec<_> = lists
            .iter()
            .filter_map(|(f, l)| chezmoi_path(f).map(|s| (s, *l)))
            .collect();
        if p.is_empty() {
            return Err("chezmoi does not manage the Brewfile or the npm package list. Use chezmoi add first.".into());
        }
        p
    } else {
        lists.into_iter().filter(|(f, _)| f.exists()).collect()
    };
    let Some(first) = paths.first() else {
        return Err(
            "There is no Brewfile and no npm package list yet. Run the backup jobs first.".into(),
        );
    };
    let dir = first.0.parent().unwrap_or(Path::new("."));
    let repo = toplevel(dir)
        .and_then(|r| r.canonicalize().ok())
        .ok_or_else(|| {
            "The Brewfile and the npm package list are not in a git repository.".to_string()
        })?;
    let files: Vec<(PathBuf, List)> = paths
        .iter()
        .filter_map(|(p, l)| Some((real(p)?.strip_prefix(&repo).ok()?.to_path_buf(), *l)))
        .collect();
    Ok(Target { repo, files })
}

/// `brew "git"` → `brew:git`. Comments and empty lines do not count.
pub fn brewfile_entries(text: &str) -> Snapshot {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let (kind, rest) = l.split_once(' ')?;
            let name = rest.trim().strip_prefix('"')?.split('"').next()?;
            Some((format!("{kind}:{name}"), String::new()))
        })
        .collect()
}

pub fn npm_entries(text: &str) -> Snapshot {
    crate::jobs::npm::parse_ls(text)
        .into_iter()
        .filter(|(k, _)| !k.is_empty())
        .collect::<Snapshot>()
}

/// The changes between two versions of one list.
pub fn changes(list: List, old: &str, new: &str) -> Vec<Entry> {
    let (a, b) = match list {
        List::Brewfile => (brewfile_entries(old), brewfile_entries(new)),
        List::Npm => {
            let ok = |t: &str| t.trim().is_empty() || serde_json::from_str::<Value>(t).is_ok();
            if !ok(old) || !ok(new) {
                return Vec::new();
            }
            (npm_entries(old), npm_entries(new))
        }
    };
    diff(&a, &b)
        .into_iter()
        .map(|c| {
            let (kind, name) = match list {
                List::Brewfile => {
                    let (k, n) = c.name.split_once(':').unwrap_or(("", &c.name));
                    (k.to_string(), n.to_string())
                }
                List::Npm => (String::new(), c.name.clone()),
            };
            Entry {
                list,
                mark: c.mark,
                kind,
                name,
                old: c.old,
                new: c.new,
            }
        })
        .collect()
}

/// Compares each file with the last commit.
pub fn plan(t: &Target) -> Result<Plan, String> {
    let mut files = Vec::new();
    let mut entries = Vec::new();
    for (rel, list) in &t.files {
        let r = rel.to_string_lossy();
        let status =
            git(&t.repo, &["status", "--porcelain", "--", &r]).map_err(|e| format!("git: {e}"))?;
        if !status.status.success() {
            return Err(String::from_utf8_lossy(&status.stderr).trim().to_string());
        }
        if status.stdout.is_empty() {
            continue;
        }
        files.push(rel.clone());
        let head = git(&t.repo, &["show", &format!("HEAD:{r}")])
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        let now = std::fs::read_to_string(t.repo.join(rel)).unwrap_or_default();
        entries.extend(changes(*list, &head, &now));
    }
    Ok(Plan {
        repo: t.repo.clone(),
        files,
        entries,
    })
}

/// `a`, `a and b`, `a, b and c`.
fn join(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn plural(n: usize) -> String {
    if n == 1 {
        "1 package".into()
    } else {
        format!("{n} packages")
    }
}

/// The titles from the most to the least specific. The first one that fits is used.
fn titles(entries: &[Entry]) -> Vec<String> {
    let mut out = Vec::new();
    if entries.is_empty() {
        out.push("chore: update the package lists".into());
        return out;
    }
    let verbs = ["upgrade", "add", "remove"];
    let by_verb: Vec<(&str, Vec<&Entry>)> = verbs
        .iter()
        .map(|v| {
            (
                *v,
                entries
                    .iter()
                    .filter(|e| e.verb() == *v)
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, list)| !list.is_empty())
        .collect();
    out.push(format!(
        "chore: {}",
        join(&entries.iter().map(Entry::phrase).collect::<Vec<_>>())
    ));
    let groups = |f: &dyn Fn(&str, &[&Entry]) -> String| {
        by_verb
            .iter()
            .map(|(v, list)| f(v, list))
            .collect::<Vec<_>>()
            .join(", ")
    };
    out.push(format!(
        "chore: {}",
        groups(&|v, list| format!(
            "{v} {}",
            join(&list.iter().map(|e| e.subject()).collect::<Vec<_>>())
        ))
    ));
    out.push(format!(
        "chore: {}",
        groups(&|v, list| format!("{v} {}", plural(list.len())))
    ));
    out.push("chore: update the package lists".into());
    out
}

pub fn title(entries: &[Entry]) -> String {
    titles(entries)
        .into_iter()
        .find(|t| t.chars().count() <= TITLE_MAX)
        .unwrap_or_else(|| "chore: update the package lists".into())
}

/// The body for a title: empty when the title names every change with its versions.
pub fn body(entries: &[Entry], title: &str) -> String {
    if entries.is_empty() || titles(entries).first().is_some_and(|t| t == title) {
        return String::new();
    }
    let mut parts = Vec::new();
    for list in [List::Npm, List::Brewfile] {
        let lines: Vec<String> = entries
            .iter()
            .filter(|e| e.list == list)
            .map(|e| format!("- {}", e.phrase()))
            .collect();
        if !lines.is_empty() {
            parts.push(format!("{}:\n{}", list.label(), lines.join("\n")));
        }
    }
    parts.join("\n\n")
}

/// Commits the files with git. Git hooks run. Returns the short hash.
pub fn commit(repo: &Path, files: &[PathBuf], title: &str, body: &str) -> Result<String, String> {
    let paths: Vec<String> = files
        .iter()
        .map(|f| f.to_string_lossy().into_owned())
        .collect();
    let fail = |o: std::process::Output| {
        let mut msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
        if msg.is_empty() {
            msg = String::from_utf8_lossy(&o.stdout).trim().to_string();
        }
        if msg.is_empty() {
            "git commit failed".to_string()
        } else {
            msg
        }
    };
    let mut add = vec!["add", "--"];
    add.extend(paths.iter().map(String::as_str));
    let out = git(repo, &add).map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(fail(out));
    }
    let mut args = vec!["commit", "-m", title];
    if !body.trim().is_empty() {
        args.extend(["-m", body]);
    }
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    let out = git(repo, &args).map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(fail(out));
    }
    let out = git(repo, &["rev-parse", "--short", "HEAD"]).map_err(|e| format!("git: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn npm(name: &str, old: &str, new: &str, mark: Mark) -> Entry {
        Entry {
            list: List::Npm,
            mark,
            kind: String::new(),
            name: name.into(),
            old: old.into(),
            new: new.into(),
        }
    }

    #[test]
    fn reads_brewfile_entries() {
        let old = "tap \"homebrew/bundle\"\nbrew \"git\"\nbrew \"jq\"\ncask \"iterm2\"\n";
        let new = "# tools\ntap \"homebrew/bundle\"\nbrew \"git\"\nbrew \"ripgrep\"\nmas \"Bear\", id: 1091189122\n";
        let c = changes(List::Brewfile, old, new);
        let phrases: Vec<_> = c.iter().map(Entry::phrase).collect();
        assert_eq!(
            phrases,
            vec![
                "add ripgrep",
                "add app Bear",
                "remove jq",
                "remove cask iterm2"
            ]
        );
    }

    #[test]
    fn reads_npm_list_changes() {
        let old = r#"{"dependencies":{"githits":{"version":"0.11.4"},"old":{"version":"1.0.0"}}}"#;
        let new = r#"{"dependencies":{"githits":{"version":"0.12.0"},"pnpm":{"version":"9.1.0"}}}"#;
        let c = changes(List::Npm, old, new);
        let phrases: Vec<_> = c.iter().map(Entry::phrase).collect();
        assert_eq!(
            phrases,
            vec![
                "upgrade githits 0.11.4 to 0.12.0",
                "add pnpm 9.1.0",
                "remove old"
            ]
        );
    }

    #[test]
    fn one_change_needs_no_body() {
        let e = vec![npm("eslint", "8.57.0", "9.0.0", Mark::Up)];
        let t = title(&e);
        assert_eq!(t, "chore: upgrade eslint 8.57.0 to 9.0.0");
        assert_eq!(body(&e, &t), "");
        // An edited title gets the full list.
        assert_eq!(
            body(&e, "chore: bump"),
            "npm globals:\n- upgrade eslint 8.57.0 to 9.0.0"
        );
    }

    #[test]
    fn long_titles_get_less_specific() {
        let e: Vec<Entry> = (0..3)
            .map(|i| {
                npm(
                    &format!("package-with-a-long-name-{i}"),
                    "1.0.0",
                    "2.0.0",
                    Mark::Up,
                )
            })
            .chain([npm("pnpm", "", "9.1.0", Mark::Add)])
            .collect();
        let t = title(&e);
        assert_eq!(t, "chore: upgrade 3 packages, add 1 package");
        let b = body(&e, &t);
        assert!(b.starts_with("npm globals:\n- upgrade package-with-a-long-name-0 1.0.0 to 2.0.0"));
        assert!(b.ends_with("- add pnpm 9.1.0"));
        let two = vec![npm("a", "1", "2", Mark::Up), npm("b", "", "1", Mark::Add)];
        assert_eq!(title(&two), "chore: upgrade a 1 to 2 and add b 1");
        assert!(title(&[]).contains("package lists"));
        let long: Vec<Entry> = ["githits", "pnpm"]
            .iter()
            .map(|n| {
                npm(
                    n,
                    "1.0.0-very-long-version",
                    "2.0.0-very-long-version",
                    Mark::Up,
                )
            })
            .chain([npm("jq", "", "", Mark::Add)])
            .collect();
        assert_eq!(title(&long), "chore: upgrade githits and pnpm, add jq");
    }

    #[test]
    fn commits_only_the_package_lists() {
        let repo = std::env::temp_dir().join(format!("dep-up-commit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join("brew")).unwrap();
        let g = |args: &[&str]| {
            let o = git(&repo, args).unwrap();
            assert!(
                o.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        };
        g(&["init", "-q"]);
        g(&["config", "user.email", "test@example.com"]);
        g(&["config", "user.name", "Test"]);
        g(&["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("brew/Brewfile"), "brew \"git\"\n").unwrap();
        std::fs::write(repo.join("other.txt"), "x").unwrap();
        g(&["add", "."]);
        g(&["commit", "-q", "-m", "init"]);
        std::fs::write(repo.join("brew/Brewfile"), "brew \"git\"\nbrew \"jq\"\n").unwrap();
        std::fs::write(repo.join("other.txt"), "changed").unwrap();
        let repo_real = repo.canonicalize().unwrap();
        let t = Target {
            repo: repo_real.clone(),
            files: vec![(PathBuf::from("brew/Brewfile"), List::Brewfile)],
        };
        let p = plan(&t).unwrap();
        assert_eq!(p.files, vec![PathBuf::from("brew/Brewfile")]);
        let ttl = title(&p.entries);
        assert_eq!(ttl, "chore: add jq");
        let hash = commit(&p.repo, &p.files, &ttl, &body(&p.entries, &ttl)).unwrap();
        assert!(!hash.is_empty());
        let st = git(&repo_real, &["status", "--porcelain"]).unwrap();
        assert_eq!(String::from_utf8_lossy(&st.stdout).trim(), "M other.txt");
        assert!(plan(&t).unwrap().files.is_empty());
        let _ = std::fs::remove_dir_all(&repo);
    }
}
