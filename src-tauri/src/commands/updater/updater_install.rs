use crate::commands::UpdateStore;
use std::io;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::Result;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn updater_install(app: AppHandle, store: State<'_, Mutex<UpdateStore>>) -> Result<()> {
  let mut s = store.lock().await;
  if !super::configuration(&app).configured {
    s.clear();
    return Err(io::Error::other("未配置自有更新公钥").into());
  }
  let update = s
    .update
    .take()
    .ok_or_else(|| io::Error::other("no update available"))?;
  let bytes = s
    .bytes
    .take()
    .ok_or_else(|| io::Error::other("no update downloaded"))?;
  if let Err(error) = update.install(&bytes) {
    let _ = app.emit("updater_error", error.to_string());
    return Err(error);
  }
  let _ = app.emit("updater_finished", ());
  app.request_restart();
  Ok(())
}
