//! Updates on two channels. Stable follows the newest full release; dev
//! follows a rolling `updater-dev` release that the release workflow points
//! at every build, pre-releases included. Dev is opt-in, in Settings, General.
//! The plugin's JavaScript check cannot pick an endpoint, so the check, the
//! download and the install run here.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const STABLE: &str = "https://github.com/lucas19919/Parzi/releases/latest/download/latest.json";
const DEV: &str = "https://github.com/lucas19919/Parzi/releases/download/updater-dev/latest.json";

#[derive(Default)]
pub struct Updates {
    pending: Mutex<Option<Update>>,
    bytes: Mutex<Option<Vec<u8>>>,
}

#[derive(Serialize)]
pub struct Found {
    version: String,
    body: Option<String>,
}

fn endpoint(dev: bool) -> &'static str {
    if dev {
        DEV
    } else {
        STABLE
    }
}

/// Looks for a newer build on the chosen channel and remembers it.
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    state: State<'_, Updates>,
    dev: bool,
) -> Result<Option<Found>, String> {
    let url = endpoint(dev).parse().map_err(|e| format!("{e}"))?;
    let found = app
        .updater_builder()
        .endpoints(vec![url])
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let info = found.as_ref().map(|u| Found {
        version: u.version.clone(),
        body: u.body.clone(),
    });
    *state.pending.lock().map_err(|e| e.to_string())? = found;
    *state.bytes.lock().map_err(|e| e.to_string())? = None;
    Ok(info)
}

/// Downloads the update found last, telling the window how far it got.
#[tauri::command]
pub async fn update_download(app: AppHandle, state: State<'_, Updates>) -> Result<(), String> {
    let update = state
        .pending
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("no update to download")?;
    let bytes = update
        .download(
            move |chunk, total| {
                let _ = app.emit(
                    "parzi://update-progress",
                    serde_json::json!({ "chunk": chunk, "total": total }),
                );
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    *state.bytes.lock().map_err(|e| e.to_string())? = Some(bytes);
    Ok(())
}

/// Installs the downloaded update. On Windows the installer closes Parzi.
#[tauri::command]
pub fn update_install(state: State<'_, Updates>) -> Result<(), String> {
    let update = state
        .pending
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or("no update to install")?;
    let bytes = state
        .bytes
        .lock()
        .map_err(|e| e.to_string())?
        .take()
        .ok_or("the update is not downloaded yet")?;
    update.install(bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stable_channel_never_follows_pre_releases() {
        assert!(endpoint(false).contains("/releases/latest/"));
        assert!(endpoint(true).contains("/updater-dev/"));
    }
}
