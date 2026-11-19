use crate::models::DownloadItem;
use crate::scheduling::dispatcher::DispatchRequest;
use crate::scheduling::download_pipeline::{DownloadRequest, DownloadSender};
use crate::scheduling::group_state::DownloadBatch;
use tauri::State;

#[tauri::command]
pub fn media_download(
  group_id: String,
  items: Vec<DownloadItem>,
  attempt_id: String,
  pipeline: State<'_, DownloadSender>,
  updater: State<'_, tokio::sync::Mutex<crate::commands::UpdateStore>>,
) -> Result<String, String> {
  let _update_guard = updater
    .try_lock()
    .map_err(|_| "应用更新正在进行，请稍后添加下载")?;
  let batch = DownloadBatch::submit_with_attempt(&group_id, Some(attempt_id));

  pipeline
    .0
    .send(DispatchRequest::Pipeline(DownloadRequest::Batch {
      group_id: group_id.clone(),
      items,
      batch,
    }))
    .map_err(|_| "下载队列已关闭".to_string())?;

  Ok(group_id)
}
