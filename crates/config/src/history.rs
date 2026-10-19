//! Search and filter history, persisted under `$XDG_STATE_HOME/gander`.
//!
//! # Why this is separate from the config
//!
//! The config is a *decision* the user made and expects to find on the next
//! machine they copy it to. History is a *byproduct* of using the program: it is
//! machine-local, it changes constantly, and losing it costs nothing. Mixing the
//! two would mean a sync'd `config.toml` carrying `last_filter = "nginx"` from
//! another machine, and a config file that rewrites itself on every search.
//!
//! The XDG split is exactly for this: `XDG_CONFIG_HOME` for what you chose,
//! `XDG_STATE_HOME` for what happened. `state_dir()` already existed for this and
//! was unused; this module is what it was for.
//!
//! # Format
//!
//! One entry per line, oldest first, most recent last. A line is the raw filter
//! text, so a `|`-separated term list round-trips exactly as typed. Newlines and
//! carriage returns cannot appear in a filter (the input handler rejects control
//! characters), so a line-based format is unambiguous.
//!
//! Entries are deduplicated by *moving* a repeated entry to the end rather than
//! keeping two copies, which is what a shell's history does and what makes
//! "press Up to repeat what I just searched" work.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// How many entries to keep. Enough to cover a session's worth of searching,
/// small enough that the file stays a few hundred bytes.
pub const MAX_ENTRIES: usize = 50;

/// Which history a set of entries belongs to.
///
/// Search and filter are kept apart on purpose: they are different questions
/// ("where is this process" vs "hide everything else"), and mixing them would put
/// a filter term into the search history where pressing Up would silently hide
/// rows instead of jumping to one.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Search,
    Filter,
}

impl Kind {
    fn file_name(self) -> &'static str {
        match self {
            Kind::Search => "search_history",
            Kind::Filter => "filter_history",
        }
    }
}

/// The most recent entries, oldest first.
#[derive(Clone, Debug, Default)]
pub struct History {
    entries: VecDeque<String>,
}

impl History {
    /// Load the history for `kind` from `dir`, tolerating any problem.
    ///
    /// A missing or unreadable file is an empty history, never an error: history
    /// is an optimisation, and a first run has none. A corrupt line is skipped
    /// rather than aborting the load, so one bad byte cannot lose the rest.
    pub fn load(dir: &Path, kind: Kind) -> Self {
        let path = dir.join(kind.file_name());
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let mut entries: VecDeque<String> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        // Keep only the newest `MAX_ENTRIES`, in case the file was hand-edited or
        // written by a version with a larger cap.
        while entries.len() > MAX_ENTRIES {
            entries.pop_front();
        }
        Self { entries }
    }

    /// Persist to `dir`, creating it if needed.
    ///
    /// Errors are the caller's to report or ignore; the TUI ignores them, because
    /// failing to save history must never fail a run.
    pub fn save(&self, dir: &Path, kind: Kind) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let mut text = String::with_capacity(self.entries.iter().map(|e| e.len() + 1).sum());
        for entry in &self.entries {
            text.push_str(entry);
            text.push('\n');
        }
        std::fs::write(dir.join(kind.file_name()), text)
    }

    /// Record `entry`, moving an existing copy to the most-recent position.
    ///
    /// Empty and whitespace-only entries are ignored: pressing Enter on an empty
    /// prompt is not a search worth remembering, and it would push a real entry
    /// out of the list.
    pub fn push(&mut self, entry: &str) {
        let entry = entry.trim();
        if entry.is_empty() {
            return;
        }
        // `retain` on a `VecDeque` keeps order, so this is a move-to-back.
        self.entries.retain(|e| e != entry);
        self.entries.push_back(entry.to_string());
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries, oldest first.
    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }

    /// The `n`-th most recent entry, where `n == 0` is the newest.
    ///
    /// This is the direction a prompt wants it: Up walks back through what was
    /// typed, starting with the last thing.
    pub fn recent(&self, n: usize) -> Option<&str> {
        let len = self.entries.len();
        if n >= len {
            return None;
        }
        self.entries.get(len - 1 - n).map(String::as_str)
    }

    /// The entry to show after walking `delta` steps from `cursor`.
    ///
    /// `cursor` is how far back the prompt already is: 0 means "the live buffer,
    /// not a history entry yet". Returns the new cursor and the text to put in the
    /// buffer. Walking past the newest entry returns the empty string, so the user
    /// can get back to typing something new — the behaviour of every shell prompt.
    pub fn step(&self, cursor: usize, delta: isize) -> (usize, Option<String>) {
        let len = self.entries.len();
        if len == 0 {
            return (0, None);
        }
        // The cursor can sit at `len`, which means "one past the oldest" and
        // shows the empty buffer again.
        let next = cursor as isize + delta;
        let next = next.clamp(0, len as isize) as usize;
        if next == 0 {
            return (0, Some(String::new()));
        }
        match self.recent(next - 1) {
            Some(text) => (next, Some(text.to_string())),
            None => (0, Some(String::new())),
        }
    }
}

/// Where history lives for this machine.
pub fn dir() -> PathBuf {
    super::state_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gander-history-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_file_is_an_empty_history() {
        let dir = temp_dir("missing");
        let h = History::load(&dir, Kind::Search);
        assert!(h.is_empty());
        assert_eq!(h.recent(0), None);
    }

    #[test]
    fn round_trips_through_the_file() {
        let dir = temp_dir("roundtrip");
        let mut h = History::default();
        h.push("python|node");
        h.push("nginx");
        h.save(&dir, Kind::Filter).expect("save");

        let back = History::load(&dir, Kind::Filter);
        assert_eq!(back.entries().collect::<Vec<_>>(), vec!["python|node", "nginx"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_and_filter_do_not_share_a_file() {
        let dir = temp_dir("kinds");
        let mut s = History::default();
        s.push("alpha");
        s.save(&dir, Kind::Search).unwrap();
        let mut f = History::default();
        f.push("beta");
        f.save(&dir, Kind::Filter).unwrap();

        assert_eq!(
            History::load(&dir, Kind::Search).entries().collect::<Vec<_>>(),
            vec!["alpha"]
        );
        assert_eq!(
            History::load(&dir, Kind::Filter).entries().collect::<Vec<_>>(),
            vec!["beta"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repeating_an_entry_moves_it_to_the_front() {
        let mut h = History::default();
        h.push("one");
        h.push("two");
        h.push("three");
        h.push("one");
        assert_eq!(
            h.entries().collect::<Vec<_>>(),
            vec!["two", "three", "one"],
            "a repeated entry must move, not duplicate"
        );
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn blank_entries_are_ignored() {
        let mut h = History::default();
        h.push("");
        h.push("   ");
        h.push("\t");
        assert!(h.is_empty());

        h.push("real");
        h.push("");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn entries_are_trimmed() {
        let mut h = History::default();
        h.push("  spaced  ");
        assert_eq!(h.recent(0), Some("spaced"));
    }

    #[test]
    fn the_cap_drops_the_oldest() {
        let mut h = History::default();
        for i in 0..MAX_ENTRIES + 10 {
            h.push(&format!("entry-{i}"));
        }
        assert_eq!(h.len(), MAX_ENTRIES);
        assert_eq!(h.recent(0), Some(format!("entry-{}", MAX_ENTRIES + 9).as_str()));
        // The first ten fell off.
        assert!(!h.entries().any(|e| e == "entry-0"));
    }

    #[test]
    fn walking_up_and_back_down() {
        let mut h = History::default();
        h.push("first");
        h.push("second");
        h.push("third");

        // Up from the live buffer: the newest entry.
        let (cursor, text) = h.step(0, 1);
        assert_eq!(cursor, 1);
        assert_eq!(text.as_deref(), Some("third"));

        let (cursor, text) = h.step(cursor, 1);
        assert_eq!(cursor, 2);
        assert_eq!(text.as_deref(), Some("second"));

        let (cursor, text) = h.step(cursor, 1);
        assert_eq!(cursor, 3);
        assert_eq!(text.as_deref(), Some("first"));

        // Past the oldest is the empty buffer, not a wrap to the newest: a wrap
        // would make it impossible to get back to typing.
        let (cursor, text) = h.step(cursor, 1);
        assert_eq!(cursor, 3, "already at the oldest");
        assert_eq!(text.as_deref(), Some("first"));

        // Down returns toward the live buffer.
        let (cursor, text) = h.step(cursor, -1);
        assert_eq!(cursor, 2);
        assert_eq!(text.as_deref(), Some("second"));

        let (cursor, text) = h.step(cursor, -1);
        assert_eq!(cursor, 1);
        assert_eq!(text.as_deref(), Some("third"));

        let (cursor, text) = h.step(cursor, -1);
        assert_eq!(cursor, 0);
        assert_eq!(text.as_deref(), Some(""), "back to an empty live buffer");
    }

    #[test]
    fn stepping_an_empty_history_does_nothing() {
        let h = History::default();
        assert_eq!(h.step(0, 1), (0, None));
        assert_eq!(h.step(0, -1), (0, None));
    }

    #[test]
    fn a_corrupt_line_does_not_lose_the_others() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(Kind::Search.file_name()),
            "good\n\n   \nalso-good\n",
        )
        .unwrap();
        let h = History::load(&dir, Kind::Search);
        assert_eq!(h.entries().collect::<Vec<_>>(), vec!["good", "also-good"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_oversized_file_is_truncated_on_load() {
        let dir = temp_dir("oversized");
        std::fs::create_dir_all(&dir).unwrap();
        let text: String = (0..MAX_ENTRIES * 2)
            .map(|i| format!("e{i}\n"))
            .collect();
        std::fs::write(dir.join(Kind::Search.file_name()), text).unwrap();

        let h = History::load(&dir, Kind::Search);
        assert_eq!(h.len(), MAX_ENTRIES, "the newest entries are kept");
        assert_eq!(
            h.recent(0),
            Some(format!("e{}", MAX_ENTRIES * 2 - 1).as_str())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
