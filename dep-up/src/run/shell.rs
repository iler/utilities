//! Runs the commands of a job.
//!
//! A command gets no keyboard input, and it runs in a new session without a terminal. A tool
//! that asks for a password therefore fails at once. It does not stop and wait. The output goes
//! to the log file of the job, and the last line goes to the board.

use crate::run::tail::LineTail;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

unsafe extern "C" {
    fn setsid() -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}
const SIGTERM: i32 = 15;
/// Keep this much output of one task in memory, for the note functions.
const CAPTURE_MAX: usize = 256 * 1024;

/// What the board can see of a running task, and how to stop it.
#[derive(Clone, Default)]
pub struct Live {
    tail: Arc<Mutex<String>>,
    pgid: Arc<Mutex<Option<u32>>>,
    cancel: Arc<AtomicBool>,
}

impl Live {
    pub fn tail(&self) -> String {
        self.tail.lock().map(|t| t.clone()).unwrap_or_default()
    }
    pub fn reset(&self) {
        self.cancel.store(false, Ordering::SeqCst);
        if let Ok(mut t) = self.tail.lock() {
            t.clear();
        }
    }
    /// Stops the command that runs now, and all commands after it.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Some(pgid) = self.pgid.lock().ok().and_then(|p| *p) {
            // The child leads its own process group, so this also stops what it started.
            unsafe {
                kill(-(pgid as i32), SIGTERM);
            }
        }
    }
    pub fn stopped(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// One command line.
#[derive(Clone, Debug, Default)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Keep the terminal of dep-up. sudo needs it: its timestamp belongs to the terminal.
    pub keep_tty: bool,
}

pub fn cmd(program: &str, args: &[&str]) -> Cmd {
    Cmd {
        program: program.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        ..Cmd::default()
    }
}

impl Cmd {
    pub fn arg(mut self, a: impl Into<String>) -> Cmd {
        self.args.push(a.into());
        self
    }
    pub fn args<I, S>(mut self, args: I) -> Cmd
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
    pub fn env(mut self, k: &str, v: impl Into<String>) -> Cmd {
        self.env.push((k.to_string(), v.into()));
        self
    }
    pub fn keep_tty(mut self) -> Cmd {
        self.keep_tty = true;
        self
    }
    pub fn display(&self) -> String {
        let mut s = self.program.clone();
        for a in &self.args {
            s.push(' ');
            if a.is_empty() || a.contains(' ') {
                s.push_str(&format!("'{a}'"));
            } else {
                s.push_str(a);
            }
        }
        s
    }
    fn command(&self, base_env: &[(String, String)]) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args).stdin(Stdio::null());
        for (k, v) in base_env.iter().chain(self.env.iter()) {
            c.env(k, v);
        }
        c
    }
}

/// The settings that every job gets.
pub fn base_env() -> Vec<(String, String)> {
    [
        // Homebrew is quieter with these. The check already updated the index.
        ("HOMEBREW_NO_ENV_HINTS", "1"),
        ("HOMEBREW_NO_AUTO_UPDATE", "1"),
        // git must fail, not wait for a login.
        ("GIT_TERMINAL_PROMPT", "0"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Runs a command for its standard output only. For snapshots: nothing goes to a log, and the
/// exit status does not matter (npm prints a valid list with exit status 1, for example).
pub fn capture(c: &Cmd) -> Option<String> {
    let out = c
        .command(&base_env())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The result of [`Sh::output`].
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

#[derive(Clone)]
struct Sink {
    log: Option<Arc<Mutex<File>>>,
    tail: Arc<Mutex<LineTail>>,
    live: Live,
    echo: bool,
    captured: Arc<Mutex<String>>,
}

impl Sink {
    fn write(&self, bytes: &[u8]) {
        if let Some(log) = &self.log
            && let Ok(mut f) = log.lock()
        {
            let _ = f.write_all(bytes);
        }
        if self.echo {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(bytes);
            let _ = out.flush();
        }
        if let Ok(mut t) = self.tail.lock() {
            t.feed(bytes);
            if let Ok(mut live) = self.live.tail.lock() {
                *live = t.current().to_string();
            }
        }
        if let Ok(mut c) = self.captured.lock() {
            c.push_str(&String::from_utf8_lossy(bytes));
            if c.len() > CAPTURE_MAX {
                let mut cut = c.len() - CAPTURE_MAX;
                while !c.is_char_boundary(cut) {
                    cut += 1;
                }
                c.drain(..cut);
            }
        }
    }

    /// Copies a stream until it ends. Returns the bytes when `keep` is set.
    fn pump(&self, mut r: impl Read, keep: bool) -> Vec<u8> {
        let mut kept = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    self.write(&buf[..n]);
                    if keep {
                        kept.extend_from_slice(&buf[..n]);
                    }
                }
            }
        }
        kept
    }
}

/// The command runner of one task.
pub struct Sh {
    sink: Sink,
    env: Vec<(String, String)>,
}

impl Sh {
    pub fn new(log: Option<&Path>, live: Live, echo: bool) -> std::io::Result<Sh> {
        let log = match log {
            Some(p) => {
                if let Some(dir) = p.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                Some(Arc::new(Mutex::new(
                    OpenOptions::new().create(true).append(true).open(p)?,
                )))
            }
            None => None,
        };
        Ok(Sh {
            sink: Sink {
                log,
                tail: Arc::new(Mutex::new(LineTail::new())),
                live,
                echo,
                captured: Arc::new(Mutex::new(String::new())),
            },
            env: base_env(),
        })
    }

    /// A runner that writes nowhere. For tests and one-off calls.
    #[cfg(test)]
    pub fn quiet() -> Sh {
        Sh::new(None, Live::default(), false).expect("no file to open")
    }

    /// Writes a line of dep-up's own to the log and the board.
    pub fn say(&self, line: &str) {
        self.sink.write(format!("{line}\n").as_bytes());
    }

    /// Writes a line to the log file only.
    pub fn note(&self, line: &str) {
        if let Some(log) = &self.sink.log
            && let Ok(mut f) = log.lock()
        {
            let _ = writeln!(f, "{line}");
        }
    }

    /// All output of this task so far (the end of it, if it is long).
    pub fn captured(&self) -> String {
        self.sink
            .captured
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    pub fn stopped(&self) -> bool {
        self.sink.live.stopped()
    }

    /// Runs a command. Its output goes to the log. A non-zero exit status is an error.
    pub fn run(&self, c: Cmd) -> Result<(), String> {
        let out = self.spawn(&c, false)?;
        match out.code {
            Some(0) => Ok(()),
            Some(n) => Err(format!("{} failed (exit {n})", short(&c))),
            None => Err(format!("{} was stopped by a signal", short(&c))),
        }
    }

    /// Runs a command and returns its output. The output also goes to the log. The exit status
    /// is in the result, and the caller decides what it means.
    pub fn output(&self, c: Cmd) -> Result<Output, String> {
        self.spawn(&c, true)
    }

    fn spawn(&self, c: &Cmd, keep: bool) -> Result<Output, String> {
        if self.stopped() {
            return Err("stopped".into());
        }
        self.note(&format!("$ {}", c.display()));
        let mut command = c.command(&self.env);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        if c.keep_tty {
            command.process_group(0);
        } else {
            // SAFETY: setsid is async-signal-safe, and nothing else runs in the child here.
            unsafe {
                command.pre_exec(|| {
                    setsid();
                    Ok(())
                });
            }
        }
        let mut child = command.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("{}: command not found", c.program),
            _ => format!("{}: {e}", c.program),
        })?;
        if let Ok(mut p) = self.sink.live.pgid.lock() {
            *p = Some(child.id());
        }
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let err_sink = self.sink.clone();
        let err_thread =
            thread::spawn(move || stderr.map(|s| err_sink.pump(s, keep)).unwrap_or_default());
        let out = stdout.map(|s| self.sink.pump(s, keep)).unwrap_or_default();
        let err = err_thread.join().unwrap_or_default();
        let status = child.wait();
        if let Ok(mut p) = self.sink.live.pgid.lock() {
            *p = None;
        }
        if self.stopped() {
            return Err("stopped".into());
        }
        let status = status.map_err(|e| format!("{}: {e}", c.program))?;
        Ok(Output {
            code: status.code(),
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
        })
    }
}

/// The program and its first argument, for error messages.
fn short(c: &Cmd) -> String {
    match c.args.first() {
        Some(a) if !a.starts_with('-') => format!("{} {a}", c.program),
        _ => c.program.clone(),
    }
}

/// True when `bin` is an executable file in the PATH.
pub fn in_path(bin: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|d| {
        std::fs::metadata(d.join(bin))
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_keeps_stdout_and_status() {
        let sh = Sh::quiet();
        let out = sh
            .output(cmd("sh", &["-c", "echo hi; echo err >&2; exit 3"]))
            .unwrap();
        assert_eq!(out.code, Some(3));
        assert_eq!(out.stdout, "hi\n");
        assert_eq!(out.stderr, "err\n");
        assert!(sh.captured().contains("hi") && sh.captured().contains("err"));
    }

    #[test]
    fn run_reports_the_exit_status() {
        let sh = Sh::quiet();
        assert!(sh.run(cmd("true", &[])).is_ok());
        assert_eq!(
            sh.run(cmd("sh", &["-c", "exit 2"])).unwrap_err(),
            "sh failed (exit 2)"
        );
        assert!(
            sh.run(cmd("dep-up-no-such-program", &[]))
                .unwrap_err()
                .contains("not found")
        );
    }

    #[test]
    fn jobs_have_no_terminal_and_no_input() {
        let sh = Sh::quiet();
        // Without a controlling terminal, /dev/tty cannot open.
        let out = sh
            .output(cmd("sh", &["-c", "cat; : < /dev/tty && echo tty"]))
            .unwrap();
        assert_eq!(out.stdout, "");
    }

    #[test]
    fn the_live_tail_follows_the_output() {
        let live = Live::default();
        let sh = Sh::new(None, live.clone(), false).unwrap();
        sh.run(cmd("printf", &["one\\ntwo\\n"])).unwrap();
        assert_eq!(live.tail(), "two");
    }

    #[test]
    fn a_stopped_runner_runs_nothing() {
        let live = Live::default();
        let sh = Sh::new(None, live.clone(), false).unwrap();
        live.stop();
        assert_eq!(sh.run(cmd("true", &[])).unwrap_err(), "stopped");
    }
}
