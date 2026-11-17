use crate::commands::UpdateStore;
#[cfg(windows)]
use crate::portable_transaction;
#[cfg(windows)]
use tauri::Manager;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

#[tauri::command]
pub async fn updater_install(
  app: AppHandle,
  store: State<'_, Mutex<UpdateStore>>,
) -> Result<(), String> {
  #[cfg(not(windows))]
  {
    let _ = (app, store);
    Err("一键更新仅支持 Windows x64 便携版".into())
  }
  #[cfg(windows)]
  {
    let mut state = store
      .try_lock()
      .map_err(|_| "更新操作正在进行，请勿重复操作")?;
    if !super::configuration(&app).configured {
      state.clear();
      return Err("当前版本不支持便携一键更新".into());
    }
    if crate::scheduling::group_state::has_pending_groups() {
      return Err("请先完成或取消下载和解析任务，再更新并重启".into());
    }
    app
      .state::<crate::SharedConfig>()
      .flush()
      .map_err(|e| e.to_string())?;
    app
      .state::<crate::SharedPreferences>()
      .flush()
      .map_err(|e| e.to_string())?;
    let manifest = state.update.as_ref().ok_or("请先检查可用更新")?;
    let raw = state.raw_manifest.as_ref().ok_or("缺少签名清单")?;
    let signature = state.signature.as_ref().ok_or("缺少更新签名")?;
    let bytes = state.bytes.as_ref().ok_or("请先下载并校验更新")?;
    let staged = portable_transaction::stage(manifest, raw, signature, bytes)?;
    portable_transaction::launch(staged)?;
    state.clear();
    // Readiness is confirmed by the replacement process after app_ready.
    app.exit(0);
    Ok(())
  }
}
