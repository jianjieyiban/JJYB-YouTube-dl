use crate::SharedConfig;
use std::env::args;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn app_ready(app: AppHandle, cfg_handle: State<'_, SharedConfig>) {
  let args: Vec<String> = args().collect();
  let is_autostart = args.contains(&"--auto-start".to_string());
  let cfg = cfg_handle.load();
  if !(cfg.system.auto_start_minimised && is_autostart) {
    app.get_webview_window("main").unwrap().show().unwrap();
    #[cfg(target_os = "macos")]
    {
      app.get_webview_window("main").unwrap().set_focus().unwrap();
    }
  }
  if let Err(error) = crate::portable_transaction::confirm_ready() {
    tracing::error!("Update readiness handshake failed: {error}");
    app.exit(1);
  }
}
