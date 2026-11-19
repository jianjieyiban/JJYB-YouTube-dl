use crate::logging::LogStoreState;
use crate::scheduling::dispatcher::DispatchRequest;
use crate::scheduling::fetch_pipeline::FetchSender;
use crate::scheduling::group_state::{
  cancel_download_group, cancel_fetch_group, DownloadCancelReceipt,
};
use tauri::State;

#[tauri::command]
pub async fn group_cancel(
  group_id: String,
  log_state: State<'_, LogStoreState>,
  fetch_sender: State<'_, FetchSender>,
  retain_partial: Option<bool>,
) -> Result<DownloadCancelReceipt, ()> {
  let retain_partial = retain_partial.unwrap_or(false);
  let pending_downloads = cancel_download_group(&group_id, retain_partial);
  cancel_fetch_group(&group_id);
  let _ = fetch_sender.0.send(DispatchRequest::Wake);

  let receipt = pending_downloads.wait().await;
  if !retain_partial {
    log_state.write().remove_group(&group_id);
  }
  Ok(receipt)
}
