//! Says in the log when the app stops answering. Two beats, so a freeze can be
//! told apart afterwards: the async runtime (agent runs, timers, most commands)
//! and the window's UI thread (buttons, dialogs, page tabs, the wallpaper).

use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::{Duration, Instant};

use tauri::AppHandle;

const RUNTIME_TICK: Duration = Duration::from_secs(1);
const UI_TICK: Duration = Duration::from_secs(2);
/// Later than this counts as a stall worth a line.
const LATE: Duration = Duration::from_millis(1500);
const UI_GIVE_UP: Duration = Duration::from_secs(5);
const UI_REPEAT: Duration = Duration::from_secs(10);
const BLOCKING_TICK: Duration = Duration::from_secs(3);

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

pub fn start(app: &AppHandle) {
    runtime_beat();
    blocking_beat();
    ui_beat(app.clone());
}

/// A task that wakes every second. If it wakes late, the runtime's worker
/// threads were all busy or blocked, so every session and command waited too.
fn runtime_beat() {
    tauri::async_runtime::spawn(async {
        loop {
            let asked = Instant::now();
            tokio::time::sleep(RUNTIME_TICK).await;
            let late = asked.elapsed().saturating_sub(RUNTIME_TICK);
            if late >= LATE {
                tracing::warn!(
                    late_ms = ms(late),
                    "async runtime stalled: its worker threads were blocked, so sessions and commands waited"
                );
            }
        }
    });
}

/// Hands the blocking pool a no-op every few seconds and times how long it takes
/// to start. Tool calls, dialogs, wallpaper and project commands all run there,
/// so when this waits, they wait too: the pool is full of jobs that do not finish.
fn blocking_beat() {
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(BLOCKING_TICK).await;
            let asked = Instant::now();
            let mut probe = tokio::task::spawn_blocking(|| ());
            if tokio::time::timeout(LATE, &mut probe).await.is_err() {
                tracing::warn!(
                    "blocking pool is full: a no-op has waited {} ms to start, so tool calls and commands that use it are waiting",
                    ms(asked.elapsed())
                );
                let _ = probe.await;
                tracing::warn!(
                    "blocking pool is free again after {} s",
                    asked.elapsed().as_secs()
                );
            }
        }
    });
}

/// Posts a no-op to the UI thread and times the answer. One request is in flight
/// at a time, so a frozen window does not pile up work.
fn ui_beat(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("parzi-watchdog".into())
        .spawn(move || loop {
            std::thread::sleep(UI_TICK);
            let (tx, rx) = channel();
            let asked = Instant::now();
            if app
                .run_on_main_thread(move || {
                    let _ = tx.send(());
                })
                .is_err()
            {
                return;
            }
            match rx.recv_timeout(UI_GIVE_UP) {
                Ok(()) => {
                    let took = asked.elapsed();
                    if took >= LATE {
                        tracing::warn!(
                            took_ms = ms(took),
                            "UI thread was slow to answer"
                        );
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    tracing::warn!(
                        "UI thread has not answered for {} s: buttons, dialogs and page tabs are stuck",
                        UI_GIVE_UP.as_secs()
                    );
                    loop {
                        match rx.recv_timeout(UI_REPEAT) {
                            Ok(()) => {
                                tracing::warn!(
                                    "UI thread answered again after {} s",
                                    asked.elapsed().as_secs()
                                );
                                break;
                            }
                            Err(RecvTimeoutError::Timeout) => tracing::warn!(
                                "UI thread still not answering after {} s",
                                asked.elapsed().as_secs()
                            ),
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("watchdog did not start: {e}");
    }
}
