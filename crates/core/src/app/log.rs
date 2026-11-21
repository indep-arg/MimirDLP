//! An activity log: what a screen did, line by line, for the panel that
//! shows it.

use std::collections::VecDeque;

use crate::log::Level;

/// How many lines are kept: enough for a whole run, and a limit for a
/// recording that goes on for hours.
pub const MAX_LOG: usize = 2000;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Counts up from the first line ever added, so the window can ask for
    /// what is new instead of the whole log every time.
    pub seq: u64,
    pub level: Level,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Log {
    lines: VecDeque<Line>,
    next_seq: u64,
    /// The last line as it arrived, and how often it has repeated, so a
    /// line yt-dlp keeps printing is collapsed instead of filling the log.
    last: Option<(Level, String)>,
    repeats: u32,
    /// Goes up with every change, a repeated line included.
    revision: u64,
}

impl Log {
    /// Adds a line, keeping the last [`MAX_LOG`]. A line repeated right
    /// away (yt-dlp says so every time it sleeps during a live recording)
    /// is counted on the line before instead.
    pub fn push(&mut self, entry: (Level, String)) {
        self.revision += 1;
        if self.last.as_ref() == Some(&entry)
            && let Some(last) = self.lines.back_mut()
        {
            self.repeats += 1;
            last.text = format!("{}  (x{})", entry.1, self.repeats + 1);
            return;
        }
        self.last = Some(entry.clone());
        self.repeats = 0;
        self.lines.push_back(Line {
            seq: self.next_seq,
            level: entry.0,
            text: entry.1,
        });
        self.next_seq += 1;
        while self.lines.len() > MAX_LOG {
            self.lines.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Line> {
        self.lines.iter()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The lines from `seq` on. Asking from the last line already seen
    /// returns that line again, which matters when it was repeated since:
    /// its text now carries the count.
    pub fn since(&self, seq: u64) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(move |line| line.seq >= seq)
    }

    pub fn latest(&self, level: Level) -> Option<&str> {
        self.lines
            .iter()
            .rev()
            .find(|line| line.level == level)
            .map(|line| line.text.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(log: &Log) -> Vec<&str> {
        log.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn a_line_yt_dlp_keeps_repeating_is_collapsed() {
        let mut log = Log::default();
        for _ in 0..3 {
            log.push((Level::Info, "[youtube] Sleeping 1.5 seconds ...".into()));
        }
        log.push((Level::Info, "[download] Destination: /tmp/v.mkv".into()));
        assert_eq!(
            texts(&log),
            [
                "[youtube] Sleeping 1.5 seconds ...  (x3)",
                "[download] Destination: /tmp/v.mkv"
            ]
        );
    }

    #[test]
    fn only_the_most_recent_lines_are_kept() {
        let mut log = Log::default();
        for n in 0..MAX_LOG + 50 {
            log.push((Level::Info, format!("line {n}")));
        }
        assert_eq!(log.len(), MAX_LOG);
        // The newest line survives, the oldest ones are gone.
        assert_eq!(texts(&log)[0], "line 50");
        assert_eq!(
            texts(&log).last().copied(),
            Some(format!("line {}", MAX_LOG + 49).as_str())
        );
    }

    /// The window asks from the last line it has: it gets that one again
    /// (its count may have changed) and everything after it.
    #[test]
    fn what_is_new_can_be_asked_for_by_line_number() {
        let mut log = Log::default();
        log.push((Level::Info, "one".into()));
        log.push((Level::Warn, "two".into()));
        let seen = log.revision();
        log.push((Level::Warn, "two".into()));
        log.push((Level::Error, "three".into()));
        assert!(log.revision() > seen);
        let new: Vec<(u64, &str)> = log.since(1).map(|l| (l.seq, l.text.as_str())).collect();
        assert_eq!(new, [(1, "two  (x2)"), (2, "three")]);
        assert_eq!(log.latest(Level::Warn), Some("two  (x2)"));
        assert_eq!(log.latest(Level::Success), None);
    }
}
