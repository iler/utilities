//! Command line parsing.

use std::path::PathBuf;

pub const HELP: &str = "dep-up - check and update all your developer tools from one list

USAGE:
    dep-up [OPTIONS]

OPTIONS:
    -a, --all            Run every job with no questions and no TUI. Use this in cron
                         or through a pipe.
    -s, --sequential     Run all jobs one after the other, not lane by lane.
    -v, --verbose        Show the raw output of each job. This implies --all and
                         --sequential.
    -o, --only <name>    Use only the jobs that match <name>, its lane or its program.
    -n, --dry-run        Show the jobs and the commands. Do not run them.
    -l, --list           List the jobs and stop.
        --cache <file>   Keep the check results in <file>. The next start reads them
                         and does not check again. R in the TUI checks again.
        --color <when>   always, never or auto, for the text output. Default: auto.
    -h, --help           Show this help.
    -V, --version        Show the version.

KEYS:
    up/down, j/k     move                    home/end   first / last
    , .              page up / page down (also PgUp / PgDn)
    space            select the job, the item or the group under the cursor
    a                select all jobs that have work / select none
    i, enter         run the selected jobs and items (or the one under the cursor)
    /                filter by name (enter keeps it, esc clears it)
    g                group by: none, lane, state
    ?                show / hide the details panel
    shift+up/down    scroll the details panel
    l                open / close the log
    c                commit the Brewfile and the npm package list with git
    R                check all jobs again (ignores the cache)
    y                copy the text that you selected with the mouse
    q, esc           quit (press two times while a job runs)

MOUSE:
    click            move the cursor to a row; click its mark to select it
    click            a key hint in the footer runs it; a click on the log opens it
    wheel            move the cursor, scroll the details panel or the log
    drag             select text in the details panel or the log, then press y";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorWhen {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub all: bool,
    pub sequential: bool,
    pub verbose: bool,
    pub only: Option<String>,
    pub dry_run: bool,
    pub list: bool,
    pub cache: Option<PathBuf>,
    pub color: ColorWhen,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            all: false,
            sequential: false,
            verbose: false,
            only: None,
            dry_run: false,
            list: false,
            cache: None,
            color: ColorWhen::Auto,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Run(Options),
    Help,
    Version,
}

fn color(v: &str) -> Result<ColorWhen, String> {
    match v {
        "auto" => Ok(ColorWhen::Auto),
        "always" => Ok(ColorWhen::Always),
        "never" => Ok(ColorWhen::Never),
        other => Err(format!(
            "unknown --color value `{other}` (use always, never or auto)"
        )),
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut o = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            match inline.clone().or_else(|| args.next()) {
                Some(v) if !v.is_empty() => Ok(v),
                _ => Err(format!("{name} needs a value")),
            }
        };
        match flag.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "-a" | "--all" => o.all = true,
            "-s" | "--sequential" => o.sequential = true,
            "-v" | "--verbose" => o.verbose = true,
            "-n" | "--dry-run" => o.dry_run = true,
            "-l" | "--list" => o.list = true,
            "-o" | "--only" => o.only = Some(value(&flag)?),
            "--cache" => o.cache = Some(PathBuf::from(value(&flag)?)),
            "--color" => o.color = color(&value(&flag)?)?,
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(Parsed::Run(o))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Parsed, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_flags_and_values() {
        let Parsed::Run(o) = run(&["-a", "--only", "brew", "--color=never", "-s"]).unwrap() else {
            panic!()
        };
        assert!(o.all && o.sequential);
        assert_eq!(o.only.as_deref(), Some("brew"));
        assert_eq!(o.color, ColorWhen::Never);
    }

    #[test]
    fn inline_values_work() {
        let Parsed::Run(o) = run(&["--only=npm", "--cache=/tmp/c.json"]).unwrap() else {
            panic!()
        };
        assert_eq!(o.only.as_deref(), Some("npm"));
        assert_eq!(o.cache, Some(PathBuf::from("/tmp/c.json")));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(run(&["--only"]).is_err());
        assert!(run(&["--color", "blue"]).is_err());
        assert!(run(&["--nope"]).is_err());
        assert_eq!(run(&["-h"]).unwrap(), Parsed::Help);
    }
}
