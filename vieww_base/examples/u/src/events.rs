//! The event log: every answer, every pass, every setting — appended, never
//! rewritten.
//!
//! This is the seed of the event-sourced core the screens front, shrunk until
//! it fits a prototype. An [`Event`] is a fact that happened; [`AppState`]'s
//! `apply` is the fold; the file is the only memory. Nothing else in the app
//! is allowed to remember anything across a restart, which is what keeps the
//! log honest: if a fact is not in the file, it did not happen. It is also
//! what makes the sketch complete *once, ever* rather than once per process —
//! the bug it exists to fix was a questionnaire that restarted at one every
//! time it ran out of bank, or the app did.
//!
//! # The format
//!
//! One event per line, first word the verb, the rest the arguments:
//!
//! ```text
//! u-log 1
//! answer 20312 0 a
//! skip 20312 1
//! ack
//! name Teja
//! tone 1
//! daily 4
//! ```
//!
//! The header line is a version, because a format this small still changes,
//! and a build that does not recognise a version must refuse the file rather
//! than fold a future format into nonsense state — and must not append to it
//! either, for the same reason. Lines that parse but are not understood are
//! skipped, which is the reasoning [`FileStorage`](vieww_platform_winit::FileStorage)
//! gives for reading a corrupt file as empty: losing an event is bad,
//! refusing to start is worse, and there is no third option that does not
//! involve asking a user about a file format.
//!
//! # Why days, and why UTC
//!
//! `answer` and `skip` carry the day they happened on, in UTC days since the
//! epoch, because the daily rhythm needs to know when a day has turned: the
//! fold resets the day's counter at the first event of a new day. UTC is a
//! compromise the prototype owns openly — a day turns at 05:30 in IST, at
//! midnight in London — and it is the one date fact `std` can produce without
//! a timezone library. The store that replaces this file owns time properly.

use std::path::{Path, PathBuf};

/// The first line of the file: a version, so a future format is refused by
/// this build rather than folded into nonsense.
const HEADER: &str = "u-log 1";

/// One fact the log records.
///
/// The training events carry the day they happened on; the settings are
/// logged as events too, so the fold — not a parallel snapshot — is what
/// restores them.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// An answer to question `ask` (`true` is the first choice, `false` the
    /// second), on `day`.
    Answered {
        /// The UTC day the answer landed.
        day: u64,
        /// Which question, as an index into the bank.
        ask: usize,
        /// The choice: `a` or `b`.
        choice: bool,
    },
    /// A pass on question `ask`: nothing noted, nothing counted.
    Skipped {
        /// The UTC day the pass happened.
        day: u64,
        /// Which question was passed on.
        ask: usize,
    },
    /// The sketch-complete moment was seen and dismissed. It shows once,
    /// ever; the log is what makes "ever" true.
    SketchSeen,
    /// What U should call you. Logged per keystroke — the last one wins, and
    /// the intermediates are the honest record of a field being typed into.
    Named(String),
    /// The voice: 0 direct, 1 warm, 2 analytical.
    Toned(usize),
    /// The decision style: 0 fast and intuitive, 1 deliberate and thorough.
    Styled(usize),
    /// Training sessions per day.
    Daily(f32),
    /// Whether a pulse is wanted when a memory lands.
    Haptics(bool),
    /// Whether the neuron field is dimmed.
    Calm(bool),
    /// Whether a fingerprint is required to open U.
    Biometric(bool),
}

impl Event {
    /// The line this event appends to the log.
    fn encode(&self) -> String {
        match self {
            Self::Answered { day, ask, choice } => {
                format!("answer {day} {ask} {}", if *choice { 'a' } else { 'b' })
            }
            Self::Skipped { day, ask } => format!("skip {day} {ask}"),
            Self::SketchSeen => "ack".to_owned(),
            Self::Named(name) => format!("name {name}"),
            Self::Toned(voice) => format!("tone {voice}"),
            Self::Styled(style) => format!("style {style}"),
            Self::Daily(daily) => format!("daily {daily}"),
            Self::Haptics(on) => format!("haptics {}", u8::from(*on)),
            Self::Calm(on) => format!("calm {}", u8::from(*on)),
            Self::Biometric(on) => format!("biometric {}", u8::from(*on)),
        }
    }

    /// One event from one line of the log, or `None` when the line is not an
    /// event this build knows. Words after the ones a verb needs are ignored:
    /// a future format's extra arguments cost an old reader nothing.
    fn decode(line: &str) -> Option<Self> {
        let line = line.trim_end();
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let mut words = rest.split_whitespace();
        match verb {
            "answer" => Some(Self::Answered {
                day: words.next()?.parse().ok()?,
                ask: words.next()?.parse().ok()?,
                choice: match words.next()? {
                    "a" => true,
                    "b" => false,
                    _ => return None,
                },
            }),
            "skip" => Some(Self::Skipped {
                day: words.next()?.parse().ok()?,
                ask: words.next()?.parse().ok()?,
            }),
            "ack" => Some(Self::SketchSeen),
            // The name is the whole rest of the line, spaces and all — the
            // one value here that can contain them.
            "name" => Some(Self::Named(rest.to_owned())),
            "tone" => Some(Self::Toned(words.next()?.parse().ok()?)),
            "style" => Some(Self::Styled(words.next()?.parse().ok()?)),
            "daily" => Some(Self::Daily(words.next()?.parse().ok()?)),
            "haptics" => Some(Self::Haptics(words.next()? == "1")),
            "calm" => Some(Self::Calm(words.next()? == "1")),
            "biometric" => Some(Self::Biometric(words.next()? == "1")),
            _ => None,
        }
    }
}

/// The day an event happens on: UTC days since the epoch.
///
/// UTC, because `std` can produce the instant but not the timezone, and a
/// prototype that owns its compromises is worth more than a dependency that
/// hides one. In IST the day turns at 05:30; in London, at midnight. The
/// store that replaces this file owns time properly.
#[must_use]
pub fn today() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() / 86_400)
}

/// An append-only event log in a file — or nowhere at all.
///
/// The file is loaded and validated once, at construction; after that the
/// log is only ever appended to, one line per event, because a mobile
/// process is killed without warning and a half-written line is the one
/// corruption this format can survive: it is skipped on the next load.
#[derive(Debug, Clone)]
pub struct Log {
    /// Where events are appended. `None` when this run keeps nothing — the
    /// screenshot tool, or a format this build does not recognise.
    file: Option<PathBuf>,
    /// The events the file held, in order, as loaded.
    history: Vec<Event>,
}

impl Log {
    /// The log at `path`: every event the file holds, in order.
    ///
    /// A file that cannot be read is an empty log — a fresh install — rather
    /// than an error, for the same reason a corrupt file is: starting is
    /// worth more than the events. A file whose header is not this build's
    /// version is also read as empty, and is never written to afterwards,
    /// because appending this format to a future one would corrupt it.
    #[must_use]
    pub fn at(path: PathBuf) -> Self {
        let (history, writable) = match std::fs::read_to_string(&path) {
            // Nothing there yet: a fresh log, and one this build may write.
            Err(_) => (Vec::new(), true),
            Ok(text) => {
                let mut lines = text.lines().map(str::trim);
                match lines.next() {
                    // An empty file is a fresh log, not a foreign one.
                    None => (Vec::new(), true),
                    Some(header) if header == HEADER => {
                        (lines.filter_map(Event::decode).collect(), true)
                    }
                    // A format this build does not know: leave it alone.
                    Some(_) => (Vec::new(), false),
                }
            }
        };
        Self {
            file: writable.then_some(path),
            history,
        }
    }

    /// A log that keeps nothing: no file is read, none is written. For the
    /// screenshot tool, whose renders must not depend on the machine drawing
    /// them, and for an Android activity that arrived before its data path
    /// did.
    #[must_use]
    pub fn volatile() -> Self {
        Self {
            file: None,
            history: Vec::new(),
        }
    }

    /// The events this log opened with, in the order they happened. This is
    /// everything a previous run left behind.
    #[must_use]
    pub fn history(&self) -> &[Event] {
        &self.history
    }

    /// Append one event.
    ///
    /// The in-memory state has usually already moved when this is called; a
    /// failed append costs a lost line, not a lost answer, and is warned
    /// about rather than propagated — there is no UI for "the disk is full"
    /// and no correct rollback for a fold that already happened.
    pub fn append(&self, event: &Event) {
        use std::io::Write;

        let Some(path) = &self.file else {
            return;
        };
        // The directory may not exist yet: an app that stores nothing should
        // leave nothing behind, so it is made here rather than at mount.
        if let Err(error) = create_parent(path) {
            complain(&format!("creating {}: {error}", path.display()));
            return;
        }
        // A file that exists but holds nothing has no header yet; the header
        // belongs on the first line ever written, so that the next load
        // recognises what it is reading.
        let fresh = path.metadata().map_or(true, |data| data.len() == 0);
        let appended = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| {
                if fresh {
                    writeln!(file, "{HEADER}")?;
                }
                writeln!(file, "{}", event.encode())
            });
        if let Err(error) = appended {
            complain(&format!("appending {}: {error}", path.display()));
        }
    }
}

/// Make the directory a path's parent lives in, if it does not yet.
fn create_parent(path: &Path) -> std::io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => std::fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

/// A warning that reaches a person: logcat on Android, where stderr is
/// discarded, and stderr everywhere else.
fn complain(message: &str) {
    #[cfg(target_os = "android")]
    log::warn!("U event log: {message}");
    #[cfg(not(target_os = "android"))]
    eprintln!("U event log: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path that is removed when the test ends.
    struct TempLog(PathBuf);

    impl TempLog {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("u-events-{name}-{}.log", std::process::id()));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }

    impl Drop for TempLog {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn an_answer_survives_being_reopened() {
        let path = TempLog::new("reopen");
        Log::at(path.0.clone()).append(&Event::Answered {
            day: 19_000,
            ask: 0,
            choice: true,
        });

        assert_eq!(
            Log::at(path.0.clone()).history(),
            [Event::Answered {
                day: 19_000,
                ask: 0,
                choice: true,
            }],
            "the whole point: this is what makes the sketch complete once, ever"
        );
    }

    #[test]
    fn the_first_line_written_is_the_version_header() {
        let path = TempLog::new("header");
        Log::at(path.0.clone()).append(&Event::Toned(2));

        let text = std::fs::read_to_string(&path.0).expect("read");
        assert!(text.starts_with("u-log 1\n"), "found instead: {text:?}");
        // And the log it loads back is the one that was written.
        assert_eq!(Log::at(path.0.clone()).history(), [Event::Toned(2)]);
    }

    #[test]
    fn a_name_with_spaces_round_trips() {
        let path = TempLog::new("name");
        Log::at(path.0.clone()).append(&Event::Named("Teja Kota".to_owned()));

        assert_eq!(
            Log::at(path.0.clone()).history(),
            [Event::Named("Teja Kota".to_owned())]
        );
    }

    #[test]
    fn a_future_version_is_neither_folded_nor_written_to() {
        let path = TempLog::new("future");
        std::fs::write(&path.0, "u-log 2\nanswer 19000 0 a\n").expect("write");

        let log = Log::at(path.0.clone());
        assert!(
            log.history().is_empty(),
            "a version we do not know folds to nothing"
        );
        log.append(&Event::Toned(1));

        let text = std::fs::read_to_string(&path.0).expect("read");
        assert_eq!(
            text, "u-log 2\nanswer 19000 0 a\n",
            "and the foreign file is left exactly as it was"
        );
    }

    #[test]
    fn a_half_written_line_is_skipped_rather_than_fatal() {
        let path = TempLog::new("truncated");
        std::fs::write(&path.0, "u-log 1\nanswer 19000 0\nskip 19001 3\n").expect("write");

        // The middle line has no choice; the lines around it survive it.
        assert_eq!(
            Log::at(path.0.clone()).history(),
            [Event::Skipped {
                day: 19_001,
                ask: 3
            }]
        );
    }

    #[test]
    fn a_volatile_log_reads_empty_and_writes_nowhere() {
        let log = Log::volatile();
        assert!(log.history().is_empty());
        // Appending to nothing is not an error; it is the point.
        log.append(&Event::Calm(true));
    }
}
