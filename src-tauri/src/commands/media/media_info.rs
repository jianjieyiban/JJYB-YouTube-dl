use crate::models::download::DownloadOverrides;
use crate::scheduling::dispatcher::DispatchRequest;
use crate::scheduling::fetch_pipeline::FetchRequest;
use crate::scheduling::group_state::FetchBatch;
use crate::FetchSender;
use tauri::State;

#[tauri::command]
pub fn media_info(
  url: String,
  id: String,
  group_id: String,
  overrides: Option<DownloadOverrides>,
  pipeline: State<'_, FetchSender>,
) -> Result<String, String> {
  pipeline
    .0
    .send(DispatchRequest::Pipeline(FetchRequest::Initial {
      batch: FetchBatch::submit(&group_id),
      url,
      id,
      overrides: Box::new(overrides),
    }))
    .map_err(|e| e.to_string())?;

  Ok(group_id)
}
