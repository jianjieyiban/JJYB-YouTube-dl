use crate::models::download::DownloadOverrides;
use crate::models::PlaylistEntry;
use crate::scheduling::dispatcher::DispatchRequest;
use crate::scheduling::fetch_pipeline::FetchRequest;
use crate::scheduling::group_state::FetchBatch;
use crate::FetchSender;
use tauri::State;

#[tauri::command]
pub fn media_playlist_expand(
  group_id: String,
  entries: Vec<PlaylistEntry>,
  overrides: Option<DownloadOverrides>,
  pipeline: State<'_, FetchSender>,
) -> Result<String, String> {
  pipeline
    .0
    .send(DispatchRequest::Pipeline(FetchRequest::Playlist {
      batch: FetchBatch::submit(&group_id),
      entries,
      overrides: Box::new(overrides),
    }))
    .map_err(|e| e.to_string())?;

  Ok(group_id)
}
