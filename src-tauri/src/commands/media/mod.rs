pub mod media_download;
pub mod media_info;
pub mod media_playlist_expand;
pub mod media_size;

pub use media_download::*;
pub use media_info::*;
pub use media_playlist_expand::*;
pub use media_size::*;

#[cfg(test)]
mod tests {
  use super::*;
  use crate::commands::UpdateStore;
  use crate::scheduling::download_pipeline::DownloadSender;
  use crate::FetchSender;
  use tauri::Manager;
  use tokio::sync::{mpsc, Mutex};

  #[test]
  fn update_lock_blocks_all_media_submissions_until_released() {
    let app = tauri::test::mock_app();
    let (fetch_tx, mut fetch_rx) = mpsc::unbounded_channel();
    let (download_tx, mut download_rx) = mpsc::unbounded_channel();
    app.manage(FetchSender(fetch_tx));
    app.manage(DownloadSender(download_tx));
    app.manage(Mutex::new(UpdateStore::default()));
    let updater = app.state::<Mutex<UpdateStore>>();
    let guard = updater.try_lock().unwrap();
    assert!(media_info(
      "https://example.com/video".into(),
      "info".into(),
      "update-lock-info".into(),
      None,
      app.state(),
      app.state(),
    )
    .unwrap_err()
    .contains("应用更新正在进行"));
    assert!(media_playlist_expand(
      "update-lock-playlist".into(),
      vec![],
      None,
      app.state(),
      app.state(),
    )
    .unwrap_err()
    .contains("应用更新正在进行"));
    let format = serde_json::from_value(serde_json::json!({ "trackType": "video" })).unwrap();
    assert!(media_size(
      "https://example.com/video".into(),
      "size".into(),
      "update-lock-size".into(),
      format,
      app.state(),
      app.state(),
    )
    .unwrap_err()
    .contains("应用更新正在进行"));
    assert!(media_download(
      "update-lock-download".into(),
      vec![],
      "attempt".into(),
      app.state(),
      app.state(),
    )
    .unwrap_err()
    .contains("应用更新正在进行"));
    assert!(fetch_rx.try_recv().is_err());
    assert!(download_rx.try_recv().is_err());
    drop(guard);
    assert!(media_info(
      "https://example.com/video".into(),
      "info".into(),
      "update-lock-released".into(),
      None,
      app.state(),
      app.state(),
    )
    .is_ok());
    let queued = fetch_rx.try_recv().unwrap();
    assert!(crate::scheduling::group_state::has_pending_fetch_group(
      "update-lock-released"
    ));
    drop(queued);
    assert!(!crate::scheduling::group_state::has_pending_fetch_group(
      "update-lock-released"
    ));
  }
}
