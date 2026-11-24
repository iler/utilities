//! The cache file of `--cache`: the last check result of each job.
//!
//! The file is read at start and written after each check. A file that dep-up cannot read is
//! left as it is and not used.

use crate::jobs::Item;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Default)]
struct Data {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    checks: BTreeMap<String, Entry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    /// Unix seconds.
    pub checked_at: u64,
    pub items: Vec<Item>,
}

pub struct CacheFile {
    path: PathBuf,
    data: Data,
    writable: bool,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl CacheFile {
    /// Opens the file. The second value is a warning for the log.
    pub fn open(path: PathBuf) -> (CacheFile, Option<String>) {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (
                    CacheFile {
                        path,
                        data: Data {
                            version: VERSION,
                            ..Data::default()
                        },
                        writable: true,
                    },
                    None,
                );
            }
            Err(e) => {
                let w = format!(
                    "cannot read the cache {}: {e}. It is not used.",
                    path.display()
                );
                return (
                    CacheFile {
                        path,
                        data: Data::default(),
                        writable: false,
                    },
                    Some(w),
                );
            }
        };
        match serde_json::from_str::<Data>(&text) {
            Ok(data) if data.version == VERSION => (
                CacheFile {
                    path,
                    data,
                    writable: true,
                },
                None,
            ),
            _ => {
                let w = format!(
                    "the cache {} has an unknown format. dep-up does not use it or change it.",
                    path.display()
                );
                (
                    CacheFile {
                        path,
                        data: Data::default(),
                        writable: false,
                    },
                    Some(w),
                )
            }
        }
    }

    pub fn get(&self, job_id: &str) -> Option<&Entry> {
        self.data.checks.get(job_id)
    }

    pub fn put(&mut self, job_id: &str, at: u64, items: &[Item]) {
        self.data.checks.insert(
            job_id.to_string(),
            Entry {
                checked_at: at,
                items: items.to_vec(),
            },
        );
        self.save();
    }

    /// Writes a temporary file and renames it, so a crash cannot leave half a file.
    fn save(&self) {
        if !self.writable {
            return;
        }
        let Ok(text) = serde_json::to_string_pretty(&self.data) else {
            return;
        };
        if let Some(dir) = self.path.parent()
            && !dir.as_os_str().is_empty()
        {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.path.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_check_results() {
        let path = std::env::temp_dir().join(format!("dep-up-cache-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let (mut c, w) = CacheFile::open(path.clone());
        assert!(w.is_none());
        c.put("brew-apps", 42, &[Item::new("git", "1", "2")]);
        let (c, _) = CacheFile::open(path.clone());
        assert_eq!(c.get("brew-apps").unwrap().checked_at, 42);
        std::fs::write(&path, "not json").unwrap();
        let (mut c, w) = CacheFile::open(path.clone());
        assert!(w.is_some());
        c.put("x", 1, &[]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");
        let _ = std::fs::remove_file(&path);
    }
}
