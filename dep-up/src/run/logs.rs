//! The log directory of a session: one log file for each job.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const KEEP_RUNS: usize = 10;

pub struct RunDir {
    pub path: PathBuf,
}

/// A local timestamp like `20261124-093015`.
fn stamp() -> String {
    Command::new("date")
        .arg("+%Y%m%d-%H%M%S")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            secs.to_string()
        })
}

impl RunDir {
    /// Makes a new directory below `root` and removes the oldest ones, so that `keep` stay.
    pub fn create(root: &Path, keep: usize) -> Result<RunDir, String> {
        let base = stamp();
        let mut path = root.join(&base);
        let mut n = 2;
        while path.exists() {
            path = root.join(format!("{base}-{n}"));
            n += 1;
        }
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
        prune(root, keep);
        Ok(RunDir { path })
    }

    pub fn log_path(&self, job_id: &str) -> PathBuf {
        self.path.join(format!("{job_id}.log"))
    }
}

/// The directory names are timestamps, so the name order is the time order.
fn prune(root: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()))
        })
        .collect();
    dirs.sort();
    if dirs.len() > keep {
        for d in &dirs[..dirs.len() - keep] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// The last `n` clean lines of a log file.
pub fn tail_lines(path: &Path, n: usize) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(16 * 1024)));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let lines = crate::run::tail::lines(&buf);
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// `~/x` for a path in the home directory.
pub fn pretty(p: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(rest) = p.strip_prefix(&home)
    {
        return format!("~/{}", rest.display());
    }
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_runs_only() {
        let root = std::env::temp_dir().join(format!("dep-up-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for n in ["20200101-000001", "20200101-000002", "20200101-000003"] {
            std::fs::create_dir_all(root.join(n)).unwrap();
        }
        let dir = RunDir::create(&root, 2).unwrap();
        let mut left: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        left.sort();
        assert_eq!(left.len(), 2);
        assert_eq!(left[0], root.join("20200101-000003"));
        assert_eq!(left[1], dir.path);
        std::fs::write(dir.log_path("x"), "a\n\x1b[31mb\x1b[0m\nc\n").unwrap();
        assert_eq!(tail_lines(&dir.log_path("x"), 2), vec!["b", "c"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
