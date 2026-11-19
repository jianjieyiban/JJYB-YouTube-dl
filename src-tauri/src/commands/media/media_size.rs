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
) -> Result<String, String> {
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
