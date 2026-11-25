//! The change list.
//!
//! dep-up does not read the output of the update commands, because that text changes between
//! tool versions. Each job instead has a snapshot function that lists the installed items with
//! their versions. dep-up takes a snapshot before and after the job and compares the two.

use std::collections::BTreeMap;

/// Item name → version.
pub type Snapshot = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    Up,
    Add,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub mark: Mark,
    pub name: String,
    pub old: String,
    pub new: String,
}

/// Updates first, then new items, then removed items. Each group is sorted by name.
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut out = Vec::new();
    for (name, new) in after {
        match before.get(name) {
            None => out.push(Change {
                mark: Mark::Add,
                name: name.clone(),
                old: String::new(),
                new: new.clone(),
            }),
            Some(old) if old != new => out.push(Change {
                mark: Mark::Up,
                name: name.clone(),
                old: old.clone(),
                new: new.clone(),
            }),
            Some(_) => {}
        }
    }
    for (name, old) in before {
        if !after.contains_key(name) {
            out.push(Change {
                mark: Mark::Remove,
                name: name.clone(),
                old: old.clone(),
                new: String::new(),
            });
        }
    }
    out.sort_by(|a, b| a.mark.cmp(&b.mark).then_with(|| a.name.cmp(&b.name)));
    out
}

/// A git commit or a content hash: it tells a human nothing.
pub fn looks_like_hash(s: &str) -> bool {
    (7..=40).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Change {
    /// The text after the name: "1.2 → 1.3", "1.0.0  (new)", "(removed)", or nothing for two
    /// hashes.
    pub fn label(&self, arrow: &str) -> String {
        match self.mark {
            Mark::Add => format!("{}  (new)", self.new).trim_start().to_string(),
            Mark::Remove => "(removed)".into(),
            Mark::Up if looks_like_hash(&self.old) && looks_like_hash(&self.new) => String::new(),
            Mark::Up => format!("{} {arrow} {}", self.old, self.new),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(pairs: &[(&str, &str)]) -> Snapshot {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn finds_updates_additions_and_removals() {
        let before = snap(&[("git", "2.43.0"), ("jq", "1.7"), ("old", "1")]);
        let after = snap(&[("git", "2.44.0"), ("jq", "1.7"), ("new", "2")]);
        let d = diff(&before, &after);
        assert_eq!(
            d.iter()
                .map(|c| (c.mark, c.name.as_str()))
                .collect::<Vec<_>>(),
            vec![(Mark::Up, "git"), (Mark::Add, "new"), (Mark::Remove, "old")]
        );
        assert_eq!(d[0].label("→"), "2.43.0 → 2.44.0");
        assert_eq!(d[1].label("→"), "2  (new)");
        assert_eq!(d[2].label("→"), "(removed)");
    }

    #[test]
    fn hashes_show_no_label() {
        let c = Change {
            mark: Mark::Up,
            name: "tdd".into(),
            old: "abc1234".into(),
            new: "def5678".into(),
        };
        assert_eq!(c.label("→"), "");
        assert!(!looks_like_hash("1.2.3"));
    }
}
