//! Reference outputs for the ported connectors' regression tests.
//!
//! The legacy connectors are gone; what they answered is kept. Each test reads a mock venue
//! through the framework and checks every value it used to compare against legacy with the
//! value recorded when both answered the same (the parity suite, green on 2026-10-18, proved
//! native and legacy equal on every one of them).
//!
//! A test names its file (`tests/golden/<name>.txt`) and checks values in order, each under
//! a label. `OTW_GOLDEN_RECORD=1` rewrites the files from what the code answers now: only
//! after a deliberate change, and the diff is the review. A large value is kept as its line
//! count, its hash and its first and last lines, so a file stays readable and small.
//!
//! A window drawn from the clock (the last hours of minute bars, a request sent "since" now)
//! answers differently as time passes: its entry is volatile (`#### ~label`), its label is
//! checked and its value is not, the test's own assertions still run. A check is volatile when
//! its label names a day within [`CLOCKED_DAYS`] of today (every window here is labelled by
//! its start), or when the test writes its label with a leading `~`. Times in labels are kept
//! as `<time>`, so a label does not follow the clock either.

use std::fmt::{Debug, Display};
use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// Values up to this size are kept whole.
const WHOLE: usize = 1_500;
const HEADER: &str = "#### ";
/// Marks an entry whose value follows the clock.
const VOLATILE: &str = "~";
const VARIES: &str = "(varies with the clock)";
/// A window starting this close to today was drawn from the clock; the fixed ones these tests
/// use start in 2024 or before.
const CLOCKED_DAYS: i64 = 500;

/// Whether a label names a day close to today.
fn clocked(label: &str) -> bool {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    let today = time::OffsetDateTime::now_utc().date();
    label.char_indices().any(|(i, _)| {
        label.get(i..i + 10).and_then(|d| time::Date::parse(d, &fmt).ok()).is_some_and(|d| (today - d).whole_days().abs() <= CLOCKED_DAYS)
    })
}

pub struct Golden {
    path: PathBuf,
    record: bool,
    got: Vec<(String, String)>,
    want: Vec<(String, String)>,
    at: usize,
}

/// A label with its times written `<time>`.
fn plain_label(label: &str) -> String {
    let mut out = String::new();
    let mut rest = label;
    // `2026-10-18 15:06:29.0 +00:00:00` and the like, as `time` prints an instant.
    while let Some(i) = rest.find(|c: char| c.is_ascii_digit()) {
        let (head, tail) = rest.split_at(i);
        out.push_str(head);
        let b = tail.as_bytes();
        let is_time = b.len() >= 10 && b[4] == b'-' && b[7] == b'-' && tail.contains("+00:00:00");
        if is_time {
            let end = tail.find("+00:00:00").map(|e| e + "+00:00:00".len()).unwrap_or(tail.len());
            out.push_str("<time>");
            rest = &tail[end..];
        } else {
            let end = tail.find(|c: char| !c.is_ascii_digit()).unwrap_or(tail.len());
            out.push_str(&tail[..end]);
            rest = &tail[end..];
        }
    }
    out.push_str(rest);
    out
}

fn parse(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        match line.strip_prefix(HEADER) {
            Some(label) => out.push((label.to_string(), String::new())),
            None => {
                if let Some((_, body)) = out.last_mut() {
                    if !body.is_empty() {
                        body.push('\n');
                    }
                    body.push_str(line);
                }
            }
        }
    }
    out
}

/// What is kept of a value: itself when small, otherwise its shape and a hash.
fn kept(text: &str) -> String {
    if text.len() <= WHOLE {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let hash = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
    let head = lines.iter().take(3).copied().collect::<Vec<_>>().join("\n");
    let tail = lines.iter().rev().take(3).rev().copied().collect::<Vec<_>>().join("\n");
    format!("{} lines, {} bytes, sha256 {hash}\n{head}\n...\n{tail}", lines.len(), text.len())
}

impl Golden {
    pub fn new(name: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.txt"));
        let record = std::env::var("OTW_GOLDEN_RECORD").is_ok_and(|v| v == "1");
        let want = if record {
            Vec::new()
        } else {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}. Record it with OTW_GOLDEN_RECORD=1", path.display()));
            parse(&text)
        };
        Golden { path, record, got: Vec::new(), want, at: 0 }
    }

    /// Check one value against the reference, in order.
    pub fn check<T: Debug + ?Sized>(&mut self, label: impl Display, value: &T) {
        let raw = label.to_string().replace('\n', " ");
        let volatile = raw.starts_with(VOLATILE) || clocked(&raw);
        let label = plain_label(raw.trim_start_matches(VOLATILE));
        let text = kept(&format!("{value:#?}"));
        if self.record {
            self.got.push(if volatile { (format!("{VOLATILE}{label}"), VARIES.to_string()) } else { (label, text) });
            return;
        }
        let Some((want_label, want)) = self.want.get(self.at) else {
            panic!("{}: no reference left for `{label}` (check {}); record again if the test grew", self.path.display(), self.at);
        };
        match want_label.strip_prefix(VOLATILE).filter(|_| volatile) {
            Some(l) => assert_eq!(l, label, "{}: check {} is not the one recorded", self.path.display(), self.at),
            None => {
                assert_eq!(want_label, &label, "{}: check {} is not the one recorded", self.path.display(), self.at);
                assert_eq!(want, &text, "{}: `{label}` differs from the reference", self.path.display());
            }
        }
        self.at += 1;
    }
}

impl Drop for Golden {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        if self.record {
            let mut out = String::new();
            for (label, text) in &self.got {
                out.push_str(HEADER);
                out.push_str(label);
                out.push('\n');
                out.push_str(text);
                out.push('\n');
            }
            if let Some(dir) = self.path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = std::fs::write(&self.path, out) {
                panic!("{}: {e}", self.path.display());
            }
            return;
        }
        assert_eq!(self.at, self.want.len(), "{}: {} reference(s) never checked", self.path.display(), self.want.len() - self.at);
    }
}
