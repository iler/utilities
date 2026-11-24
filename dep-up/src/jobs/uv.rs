//! Python tools that `uv tool install` installed.

use super::{Ctx, Item, Job, Need};
use crate::run::shell::{Sh, capture, cmd};
use crate::run::snapshot::Snapshot;

pub fn job() -> Job {
    let mut j = Job::base("uv", "uv tools", "uv", Need::Bin("uv"), update);
    j.command = "uv tool upgrade --all".into();
    j.check_text = "uv tool list --outdated";
    j.check = Some(check);
    j.per_item = true;
    j.snapshot = Some(snapshot);
    j
}

/// `uv tool list` prints `ruff v0.6.0`, then `- ruff` for each program. With `--outdated` the
/// tool line ends in `[latest: 0.7.0]`.
fn tool_line(line: &str) -> Option<(String, String, Option<String>)> {
    if line.starts_with('-') || line.starts_with(' ') {
        return None;
    }
    let mut words = line.split_whitespace();
    let name = words.next()?;
    let version = words.next()?.strip_prefix('v')?;
    let latest = line
        .split("[latest:")
        .nth(1)
        .and_then(|r| r.split(']').next())
        .map(|v| v.trim().trim_start_matches('v').to_string());
    Some((name.to_string(), version.to_string(), latest))
}

pub fn parse_outdated(text: &str) -> Vec<Item> {
    text.lines()
        .filter_map(tool_line)
        .filter_map(|(n, v, latest)| latest.filter(|l| *l != v).map(|l| Item::new(&n, &v, &l)))
        .collect()
}

pub fn parse_list(text: &str) -> Snapshot {
    text.lines()
        .filter_map(tool_line)
        .map(|(n, v, _)| (n, v))
        .collect()
}

fn check(_: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let out = sh.output(cmd("uv", &["tool", "list", "--outdated"]))?;
    if !out.ok() {
        return Err(format!(
            "uv tool list failed (exit {})",
            out.code.unwrap_or(-1)
        ));
    }
    Ok(parse_outdated(&out.stdout))
}

fn update(_: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    sh.run(match items {
        None => cmd("uv", &["tool", "upgrade", "--all"]),
        Some(items) => cmd("uv", &["tool", "upgrade"]).args(super::args(items)),
    })
}

fn snapshot(_: &Ctx) -> Snapshot {
    capture(&cmd("uv", &["tool", "list"]))
        .map(|o| parse_list(&o))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_outdated_tools() {
        let text = "ruff v0.6.0 [latest: 0.7.0]\n- ruff\nblack v24.1.0\n- black\n";
        assert_eq!(
            parse_outdated(text),
            vec![Item::new("ruff", "0.6.0", "0.7.0")]
        );
        let s = parse_list(text);
        assert_eq!(s.get("black").map(String::as_str), Some("24.1.0"));
        assert_eq!(s.len(), 2);
    }
}
