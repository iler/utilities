mod cache;
mod cli;
mod dotfiles;
mod engine;
mod jobs;
mod plain;
mod run;
mod style;
mod summary;
mod tui;

use cli::{HELP, Options, Parsed};
use engine::Engine;
use jobs::{Ctx, Job, registry};
use run::logs::{KEEP_RUNS, RunDir, pretty};
use std::io::IsTerminal;
use std::process::ExitCode;
use style::Paint;

fn print_jobs(jobs: &[Job], p: Paint, commands: bool) {
    let g = style::glyphs();
    let w = jobs
        .iter()
        .map(|j| j.name.chars().count())
        .max()
        .unwrap_or(10);
    println!("\n  {}\n", p.bold("Jobs"));
    for j in jobs {
        let have = if j.need.present() {
            p.green(g.ok)
        } else {
            p.grey(&format!("{} {} is not installed", g.miss, j.need.label()))
        };
        let name = format!("{:<w$}", j.name);
        let lane = format!("lane {:<6}", j.lane);
        println!("  {} {} {have}", p.cyan(&name), p.grey(&lane));
        if commands {
            println!("  {:<w$} {}", "", p.dim(&format!("runs:  {}", j.command)));
            if j.check.is_some() {
                println!(
                    "  {:<w$} {}",
                    "",
                    p.dim(&format!("check: {}", j.check_text))
                );
            }
        }
    }
    println!();
}

fn start(o: Options) -> Result<bool, String> {
    let ctx = Ctx::from_env();
    let mut jobs = registry(&ctx);
    if let Some(want) = &o.only {
        let w = want.to_lowercase();
        jobs.retain(|j| j.matches(&w));
        if jobs.is_empty() {
            return Err(format!("no job matches \"{want}\""));
        }
    }
    let paint = Paint::new(o.color);
    if o.list || o.dry_run {
        print_jobs(&jobs, paint, o.dry_run);
        return Ok(false);
    }
    // -v shows the raw output, so it runs without the TUI and one job at a time.
    let all = o.all || o.verbose;
    if !all && !(std::io::stdout().is_terminal() && std::io::stdin().is_terminal()) {
        return Err(
            "the interactive list needs a terminal. Use --all to run every job without it".into(),
        );
    }
    let root = ctx.log_root();
    let rundir = RunDir::create(&root, KEEP_RUNS)
        .map_err(|e| format!("{e} (set DEP_UP_LOG_DIR to change {})", pretty(&root)))?;
    let mut engine = Engine::new(jobs, ctx, rundir, o.sequential || o.verbose, o.verbose);
    if all {
        return Ok(plain::run(engine, paint));
    }
    if let Some(c) = o.cache {
        engine.use_cache(c);
    }
    tui::run(engine, paint).map_err(|e| format!("terminal error: {e}"))
}

fn main() -> ExitCode {
    let o = match cli::parse(std::env::args().skip(1)) {
        Ok(Parsed::Help) => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Version) => {
            println!("dep-up {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Run(o)) => o,
        Err(e) => {
            eprintln!("dep-up: {e}\n\nRun dep-up --help to see the options.");
            return ExitCode::from(2);
        }
    };
    match start(o) {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(1),
        Err(e) => {
            eprintln!("dep-up: {e}.");
            ExitCode::from(2)
        }
    }
}
