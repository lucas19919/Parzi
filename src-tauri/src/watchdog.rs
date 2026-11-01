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

pub fn start(app: &AppHandle) {
    runtime_beat();
    ui_beat(app.clone());
}

/// A task that wakes every second. If it wakes late, the runtime's few worker
/// threads were all busy or blocked, so every session and command waited too.
fn runtime_beat() {
    tauri::async_runtime::spawn(async {
        loop {
            let asked = Instant::now();
            tokio::time::sleep(RUNTIME_TICK).await;
            let late = asked.elapsed().saturating_sub(RUNTIME_TICK);
            if late >= LATE {
                tracing::warn!(
                    late_ms = late.as_millis() as u64,
                    "async runtime stalled: its worker threads were blocked, so sessions and commands waited"
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
                            took_ms = took.as_millis() as u64,
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
