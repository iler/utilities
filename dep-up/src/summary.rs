//! The text report at the end of a session: the change list of each job, the end of the log of
//! each failed job, and the log directory.

use crate::engine::Engine;
use crate::run::logs::{pretty, tail_lines};
use crate::run::snapshot::Mark;
use crate::style::{Paint, duration, glyphs};

/// The change list. Returns false when nothing changed.
pub fn changes(e: &Engine, p: Paint, out: &mut String) -> bool {
    let g = glyphs();
    let mut any = false;
    for (i, job) in e.jobs.iter().enumerate() {
        let list = e.states[i].session_changes();
        if list.is_empty() {
            continue;
        }
        any = true;
        out.push_str(&format!(
            "\n  {} {}\n",
            p.bold(job.name),
            p.grey(&format!("{} changed", list.len()))
        ));
        let w = list
            .iter()
            .map(|c| c.name.chars().count())
            .max()
            .unwrap_or(0)
            .min(34);
        for c in &list {
            let (mark, color): (&str, fn(&Paint, &str) -> String) = match c.mark {
                Mark::Up => (g.up, Paint::cyan),
                Mark::Add => (g.add, Paint::green),
                Mark::Remove => (g.del, Paint::yellow),
            };
            let label = c.label(g.arrow);
            if label.is_empty() {
                out.push_str(&format!("    {} {}\n", color(&p, mark), c.name));
            } else {
                out.push_str(&format!(
                    "    {} {:<w$} {}\n",
                    color(&p, mark),
                    c.name,
                    p.dim(&label)
                ));
            }
        }
    }
    any
}

/// The full report. Returns true when a job failed. `count_prep` is false in the TUI, where
/// the check runs the prep jobs (`brew update`) by itself.
pub fn report(e: &Engine, p: Paint, count_prep: bool) -> (String, bool) {
    let g = glyphs();
    let mut out = String::new();
    let failed = e.any_failed();
    let total = p.grey(&format!("total {}", duration(e.started.elapsed())));
    let ran = e.any_run(count_prep);
    if failed {
        out.push_str(&format!(
            "\n  {}   {total}\n",
            p.red(&p.bold("Some updates failed."))
        ));
    } else if !ran {
        out.push_str(&format!("\n  {}   {total}\n", p.grey("No job ran.")));
    } else {
        out.push_str(&format!(
            "\n  {}   {total}\n",
            p.green(&p.bold("All updates are done."))
        ));
    }
    if ran && !changes(e, p, &mut out) {
        let text = if failed {
            "Nothing changed."
        } else {
            "Nothing changed. Every tool is up to date."
        };
        out.push_str(&format!("\n  {}\n", p.grey(text)));
    }
    for (i, job) in e.jobs.iter().enumerate() {
        let Some(err) = e.states[i].last.as_ref().and_then(|r| r.error.clone()) else {
            continue;
        };
        out.push_str(&format!(
            "\n  {}\n",
            p.red(&format!("{} {} failed: {err}", g.fail, job.name))
        ));
        out.push_str(&format!("  {}\n", p.grey(&"-".repeat(40))));
        for l in tail_lines(&e.log_path(i), 25) {
            out.push_str(&format!("  {l}\n"));
        }
    }
    match &e.dotfiles {
        Some(Ok(src)) => out.push_str(&format!(
            "\n  {}\n",
            p.grey(&format!(
                "Dotfiles: chezmoi re-add is done. Check {} and commit.",
                pretty(src)
            ))
        )),
        Some(Err(err)) => out.push_str(&format!("\n  {}\n", p.yellow(&format!("Dotfiles: {err}")))),
        None => {}
    }
    out.push_str(&format!(
        "\n  {}\n",
        p.grey(&format!("Logs: {}", pretty(&e.rundir.path)))
    ));
    (out, failed)
}
