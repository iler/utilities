//! `--all`: run every job with no questions and print plain lines. For cron and pipes.

use crate::engine::Engine;
use crate::style::{Paint, glyphs};
use std::time::Duration;

/// Returns true when a job failed.
pub fn run(mut e: Engine, p: Paint) -> bool {
    let g = glyphs();
    let mut batch = Vec::new();
    for (i, job) in e.jobs.iter().enumerate() {
        if e.states[i].missing {
            println!(
                "{}",
                p.grey(&format!(
                    "{} {} ({} is not installed)",
                    g.miss,
                    job.name,
                    job.need.label()
                ))
            );
        } else {
            batch.push((i, None));
        }
    }
    if !e.echo {
        let names: Vec<&str> = batch.iter().map(|(i, _)| e.jobs[*i].name).collect();
        let what = if names.len() == 1 { "job" } else { "jobs" };
        println!(
            "{}",
            p.bold(&format!("Run {} {what}: {}", names.len(), names.join(", ")))
        );
    }
    e.enqueue_runs(batch, false);
    let mut shown = e.log.len();
    loop {
        e.pump();
        e.wait(Duration::from_millis(200));
        for line in &e.log[shown..] {
            println!("{}", color_line(line, p));
        }
        shown = e.log.len();
        if e.idle() {
            break;
        }
    }
    let (text, failed) = crate::summary::report(&e, p, true);
    print!("{text}");
    failed
}

fn color_line(line: &str, p: Paint) -> String {
    let g = glyphs();
    if line.starts_with(g.ok) {
        p.green(line)
    } else if line.starts_with(g.fail) {
        p.red(line)
    } else if line.starts_with('⚠') || line.starts_with(g.skip) {
        p.yellow(line)
    } else {
        line.to_string()
    }
}
