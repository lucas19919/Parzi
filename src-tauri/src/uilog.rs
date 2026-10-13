//! The window's own uncaught errors, written to the app log. The UI calls
//! `ui_log` from its global error handlers: a loop that throws must not
//! fill the disk, so entries are clipped and rate-limited.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

const MESSAGE_MAX: usize = 2 * 1024;
const STACK_MAX: usize = 4 * 1024;
/// At most this many entries per window; the rest are counted.
const BURST: u32 = 20;
const WINDOW: Duration = Duration::from_secs(60);

/// Counts entries per window and how many were dropped.
#[derive(Debug)]
struct Throttle {
    since: Instant,
    taken: u32,
    dropped: u32,
}

#[derive(Debug, PartialEq, Eq)]
enum Admit {
    /// Write it; `dropped` entries were skipped since the last one written.
    Yes {
        dropped: u32,
    },
    No,
}

impl Throttle {
    fn new(now: Instant) -> Self {
        Self {
            since: now,
            taken: 0,
            dropped: 0,
        }
    }

    fn admit(&mut self, now: Instant) -> Admit {
        if now.duration_since(self.since) >= WINDOW {
            self.since = now;
            self.taken = 0;
        }
        if self.taken >= BURST {
            self.dropped = self.dropped.saturating_add(1);
            return Admit::No;
        }
        self.taken += 1;
        Admit::Yes {
            dropped: std::mem::take(&mut self.dropped),
        }
    }
}

static THROTTLE: Mutex<Option<Throttle>> = Mutex::new(None);

/// At most `max` bytes, cut on a character boundary, on one line: control
/// characters (newlines in a stack too) are escaped, so a message can't
/// forge log lines of its own.
fn clip(text: &str, max: usize) -> String {
    let head = |s: &str| {
        let mut end = s.len().min(max);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        end
    };
    let cut = head(text);
    let escaped: String = text[..cut].escape_debug().collect();
    // Escapes can make it longer again.
    let end = head(&escaped);
    let mut out = escaped[..end].to_string();
    if cut < text.len() || end < escaped.len() {
        out.push_str("...");
    }
    out
}

/// Writes one uncaught UI error (`level` "error", anything else is a
/// warning) with its stack, if it has one.
#[tauri::command]
pub fn ui_log(level: String, message: String, stack: Option<String>) {
    let admit = THROTTLE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(|| Throttle::new(Instant::now()))
        .admit(Instant::now());
    let Admit::Yes { dropped } = admit else {
        return;
    };
    if dropped > 0 {
        tracing::warn!(target: "parzi::ui", "{dropped} more UI error(s) were not logged");
    }
    let message = clip(&message, MESSAGE_MAX);
    let stack = stack
        .filter(|s| !s.trim().is_empty())
        .map(|s| clip(&s, STACK_MAX))
        .unwrap_or_default();
    if level.eq_ignore_ascii_case("error") {
        tracing::error!(target: "parzi::ui", stack = %stack, "{message}");
    } else {
        tracing::warn!(target: "parzi::ui", stack = %stack, "{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loop_of_errors_is_cut_off_and_counted() {
        let t0 = Instant::now();
        let mut t = Throttle::new(t0);
        for _ in 0..BURST {
            assert_eq!(t.admit(t0), Admit::Yes { dropped: 0 });
        }
        for _ in 0..5 {
            assert_eq!(t.admit(t0 + Duration::from_secs(1)), Admit::No);
        }
        // The next window writes again and says how many were skipped.
        assert_eq!(t.admit(t0 + WINDOW), Admit::Yes { dropped: 5 });
        assert_eq!(t.admit(t0 + WINDOW), Admit::Yes { dropped: 0 });
    }

    #[test]
    fn entries_are_clipped_to_one_line() {
        assert_eq!(clip("boom", 100), "boom");
        assert_eq!(
            clip("at a\nat b\r\n", 100),
            "at a\\nat b\\r\\n",
            "no forged log lines"
        );
        let long = "é".repeat(3000);
        let cut = clip(&long, MESSAGE_MAX);
        assert!(cut.ends_with("..."));
        assert!(cut.len() <= MESSAGE_MAX + 3, "{}", cut.len());
        assert_eq!(clip("", 10), "");
        let lines = clip(&"\n".repeat(MESSAGE_MAX), MESSAGE_MAX);
        assert!(lines.len() <= MESSAGE_MAX + 3, "{}", lines.len());
    }
}
