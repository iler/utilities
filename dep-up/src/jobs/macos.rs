//! macOS system updates.
//!
//! Most updates need root. A job gets no keyboard input, so it uses sudo only when sudo needs
//! no password. The TUI can ask for the password before the job starts. dep-up never restarts
//! the machine: an update that needs a restart stays ready, and macOS asks later.

use super::{Ctx, Item, Job, Need, note_last_line};
use crate::run::shell::{Sh, cmd};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const ID: &str = "macos";

pub fn job() -> Job {
    let mut j = Job::base(
        ID,
        "macOS updates",
        "macos",
        Need::Bin("softwareupdate"),
        update,
    );
    j.command = "softwareupdate --install --all --agree-to-license".into();
    j.check_text = "softwareupdate --list";
    j.check = Some(check);
    j.per_item = true;
    j.note = Some(note_last_line);
    j
}

/// True when sudo works without a password now.
pub fn sudo_ready() -> bool {
    Command::new("sudo")
        .args(["-n", "true"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The list looks like this:
///
/// ```text
/// * Label: macOS Sequoia 15.1-24B83
///     Title: macOS Sequoia 15.1, Version: 15.1, Size: 5000000KiB, Recommended: YES, Action: restart,
/// ```
pub fn parse_list(text: &str) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(label) = t.strip_prefix("* Label:") {
            let label = label.trim();
            items.push(Item::new(label, "", "").id(label));
        } else if let Some(fields) = t.strip_prefix("Title:")
            && let Some(item) = items.last_mut()
        {
            let mut title = String::new();
            for (i, part) in fields.split(',').enumerate() {
                let part = part.trim();
                if i == 0 {
                    title = part.to_string();
                } else if let Some(v) = part.strip_prefix("Version:") {
                    item.latest = v.trim().to_string();
                } else if part
                    .strip_prefix("Action:")
                    .is_some_and(|a| a.trim() == "restart")
                {
                    item.note = "restart".into();
                }
            }
            if !title.is_empty() {
                item.name = title;
            }
        }
    }
    items
}

fn check(_: &Ctx, sh: &Sh) -> Result<Vec<Item>, String> {
    let out = sh.output(cmd("softwareupdate", &["--list"]))?;
    if !out.ok() {
        return Err(format!(
            "softwareupdate --list failed (exit {})",
            out.code.unwrap_or(-1)
        ));
    }
    Ok(parse_list(&out.stdout))
}

/// Keeps the sudo timestamp fresh while a long install runs.
fn keep_sudo_alive(stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            for _ in 0..60 {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            let _ = Command::new("sudo")
                .args(["-n", "-v"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    });
}

fn update(_: &Ctx, sh: &Sh, items: Option<&[Item]>) -> Result<(), String> {
    let root = sudo_ready();
    let mut c = if root {
        cmd("sudo", &["-n", "softwareupdate", "--install"]).keep_tty()
    } else {
        sh.say("sudo needs a password. Install only the updates that need no root.");
        cmd("softwareupdate", &["--install"])
    };
    c = match items {
        None => c.arg("--all"),
        Some(items) => c.args(super::args(items)),
    };
    c = c.arg("--agree-to-license");
    let stop = Arc::new(AtomicBool::new(false));
    if root {
        keep_sudo_alive(stop.clone());
    }
    let result = sh.run(c);
    stop.store(true, Ordering::SeqCst);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_update_list() {
        let items = parse_list(include_str!("../../tests/fixtures/softwareupdate_list.txt"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "Command Line Tools for Xcode");
        assert_eq!(items[0].id, "Command Line Tools for Xcode-16.1");
        assert_eq!(items[0].latest, "16.1");
        assert_eq!(items[0].note, "");
        assert_eq!(items[1].name, "macOS Sequoia 15.1");
        assert_eq!(items[1].note, "restart");
        assert!(parse_list("Software Update Tool\n\nFinding available software\n").is_empty());
    }
}
