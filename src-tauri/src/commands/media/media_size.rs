use crate::models::download::FormatOptions;
use crate::scheduling::dispatcher::DispatchRequest;
use crate::scheduling::fetch_pipeline::{FetchRequest, FetchSender};
use crate::scheduling::group_state::FetchBatch;
use tauri::State;

#[tauri::command]
pub fn media_size(
  url: String,
  id: String,
  group_id: String,
  format: FormatOptions,
  pipeline: State<'_, FetchSender>,
  updater: State<'_, tokio::sync::Mutex<crate::commands::UpdateStore>>,
) -> Result<String, String> {
  let _update_guard = updater
    .try_lock()
    .map_err(|_| "应用更新正在进行，请稍后添加解析任务")?;
  pipeline
    .0
    .send(DispatchRequest::Pipeline(FetchRequest::Size {
      batch: FetchBatch::submit(&group_id),
      url,
      id,
      format,
    }))
    .map_err(|e| e.to_string())?;

  Ok(group_id)
}
