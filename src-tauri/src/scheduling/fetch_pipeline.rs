use crate::models::download::{DownloadOverrides, FormatOptions};
use crate::models::payloads::MediaAddWithFormatPayload;
use crate::models::{MediaAddPayload, MediaFatalPayload, PlaylistEntry};
use crate::runners::ytdlp_info::{run_ytdlp_info_fetch, YtdlpInfoFetchError};
use crate::scheduling::group_state::FetchBatch;
use crate::{
  models::{ParsedMedia, ParsedPlaylist},
  scheduling::concurrency::DynamicSemaphore,
  scheduling::dispatcher::{DispatchEntry, DispatchRequest, GenericDispatcher},
};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

#[derive(Clone)]
pub struct FetchSender(pub UnboundedSender<DispatchRequest<FetchRequest>>);

#[derive(Clone)]
pub enum FetchRequest {
  Initial {
    batch: Arc<FetchBatch>,
    id: String,
    url: String,
    overrides: Box<Option<DownloadOverrides>>,
  },
  Playlist {
    batch: Arc<FetchBatch>,
    entries: Vec<PlaylistEntry>,
    overrides: Box<Option<DownloadOverrides>>,
  },
  Size {
    batch: Arc<FetchBatch>,
    id: String,
    url: String,
    format: FormatOptions,
  },
  SizePlaylist {
    batch: Arc<FetchBatch>,
    playlist: ParsedPlaylist,
    format: FormatOptions,
  },
}

#[derive(Clone)]
pub struct FetchEntry {
  batch: Arc<FetchBatch>,
  pub id: String,
  pub url: String,
  pub total: usize,
  pub format: Option<FormatOptions>,
  pub overrides: Option<DownloadOverrides>,
}

impl DispatchEntry for FetchEntry {
  fn group_id(&self) -> &String {
    self.batch.group_id()
  }
  fn is_running(&self) -> bool {
    self.batch.is_running()
  }
  fn group_key(&self) -> Option<&String> {
    None
  }
  fn set_numbering(&mut self, _autonumber: u64, _group_autonumber: Option<u64>) {}
}

pub fn setup_fetch_dispatcher(
  app: &AppHandle,
  sem: Arc<DynamicSemaphore>,
) -> GenericDispatcher<FetchRequest> {
  GenericDispatcher::start(
    app.clone(),
    sem,
    expand_fetch_request,
    |tx: UnboundedSender<DispatchRequest<FetchRequest>>, app: AppHandle, entry: FetchEntry| async move {
      handle_fetch_entry(tx, app, entry).await;
    },
  )
}

fn expand_fetch_request(req: FetchRequest) -> Vec<FetchEntry> {
  match req {
    FetchRequest::Initial {
      batch,
      id,
      url,
      overrides,
    } => {
      vec![FetchEntry {
        batch,
        id,
        url,
        total: 1,
        format: None,
        overrides: *overrides,
      }]
    }
    FetchRequest::Playlist {
      batch,
      entries,
      overrides,
    } => {
      let total = entries.len();
      entries
        .into_iter()
        .map(|e| FetchEntry {
          batch: batch.clone(),
          id: Uuid::new_v4().to_string(),
          url: e.video_url,
          total,
          format: None,
          overrides: *overrides.clone(),
        })
        .collect()
    }
    FetchRequest::Size {
      batch,
      id,
      url,
      format,
    } => {
      vec![FetchEntry {
        batch,
        id,
        url,
        total: 1,
        format: Some(format),
        overrides: None,
      }]
    }
    FetchRequest::SizePlaylist {
      batch,
      playlist,
      format,
    } => {
      let total = playlist.entries.len();
      playlist
        .entries
        .into_iter()
        .map(|e| FetchEntry {
          batch: batch.clone(),
          id: Uuid::new_v4().to_string(),
          url: e.video_url,
          total,
          format: Some(format.clone()),
          overrides: None,
        })
        .collect()
    }
  }
}

async fn handle_fetch_entry(
  tx: UnboundedSender<DispatchRequest<FetchRequest>>,
  app: AppHandle,
  entry: FetchEntry,
) {
  let FetchEntry {
    batch,
    id,
    url,
    total,
    format,
    overrides,
  } = entry;
  if !batch.is_running() {
    return;
  }
  let group_id = batch.group_id().clone();

  let result = run_ytdlp_info_fetch(
    &app,
    id.clone(),
    group_id.clone(),
    &url,
    format.clone(),
    overrides.clone(),
  )
  .await;

  if !batch.is_running() {
    return;
  }

  let result = match result {
    Ok(v) => v,
    Err(e) => {
      tracing::warn!(
        fetch_id = %id,
        group_id = %group_id,
        url = %url,
        error = %e,
        "run_ytdlp_info_fetch failed"
      );
      if should_report_to_sentry(&e) {
        sentry::capture_error(&e);
      }

      None
    }
  };

  match result {
    Some(ParsedMedia::Single(single)) => {
      if let Some(format) = format {
        let payload = MediaAddWithFormatPayload {
          group_id: group_id.clone(),
          total,
          item: single,
          format,
        };
        let _ = app.emit("media_size", payload);
      } else {
        let payload = MediaAddPayload {
          group_id: group_id.clone(),
          total,
          item: single,
        };
        let _ = app.emit("media_add", payload);
      }
    }
    Some(ParsedMedia::Playlist(pl)) => {
      if let Some(format) = format {
        let _ = tx.send(DispatchRequest::Pipeline(FetchRequest::SizePlaylist {
          batch,
          playlist: pl,
          format,
        }));
      } else {
        let payload = MediaAddPayload {
          group_id,
          total: pl.entries.len(),
          item: pl,
        };
        let _ = app.emit("media_add", payload);
      }
    }
    Some(ParsedMedia::Livestream(_)) => {
      let payload =
        MediaFatalPayload::internal(group_id.clone(), id, "Livestreams unsupported".into(), None);
      let _ = app.emit("media_fatal", payload);
    }
    None => {
      // Do nothing if no parsed result is returned. The events have already been sent.
    }
  }
}

fn should_report_to_sentry(err: &YtdlpInfoFetchError) -> bool {
  matches!(
    err,
    YtdlpInfoFetchError::InvalidDiagnosticRules(_)
      | YtdlpInfoFetchError::RunnerFailed(_)
      | YtdlpInfoFetchError::ParseFailed(_)
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::TrackType;
  use crate::scheduling::group_state::{cancel_fetch_group, has_pending_fetch_group};
  use tokio::sync::mpsc;

  fn initial(group_id: &str) -> FetchRequest {
    FetchRequest::Initial {
      batch: FetchBatch::submit(group_id),
      id: "media".into(),
      url: "https://example.com/video".into(),
      overrides: Box::new(None),
    }
  }

  #[test]
  fn empty_playlist_expansion_releases_fetch_group() {
    let group_id = "fetch-empty-playlist";
    assert!(expand_fetch_request(FetchRequest::Playlist {
      batch: FetchBatch::submit(group_id),
      entries: vec![],
      overrides: Box::new(None),
    })
    .is_empty());
    assert!(!has_pending_fetch_group(group_id));
  }

  #[test]
  fn fetch_group_stays_busy_until_queued_request_and_last_entry_exit() {
    let group_id = "fetch-queued-and-running";
    let (tx, mut rx) = mpsc::unbounded_channel();
    assert!(tx.send(initial(group_id)).is_ok());
    assert!(has_pending_fetch_group(group_id));

    let first = expand_fetch_request(rx.try_recv().unwrap()).pop().unwrap();
    let second = expand_fetch_request(initial(group_id)).pop().unwrap();
    drop(first);
    assert!(has_pending_fetch_group(group_id));
    drop(second);
    assert!(!has_pending_fetch_group(group_id));
  }

  #[test]
  fn closed_dispatch_channel_does_not_leave_fetch_group_busy() {
    let group_id = "fetch-send-failed";
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);
    assert!(tx.send(initial(group_id)).is_err());
    assert!(!has_pending_fetch_group(group_id));
  }

  #[test]
  fn cancelling_and_draining_old_entries_does_not_retire_resubmission() {
    let group_id = "fetch-cancel-resubmit";
    cancel_fetch_group(group_id);
    assert!(!has_pending_fetch_group(group_id));
    let old = expand_fetch_request(initial(group_id)).pop().unwrap();
    cancel_fetch_group(group_id);
    assert!(has_pending_fetch_group(group_id));
    let new = expand_fetch_request(initial(group_id)).pop().unwrap();
    assert!(!old.is_running());
    drop(old);
    assert!(new.is_running());
    assert!(has_pending_fetch_group(group_id));
    drop(new);
    assert!(!has_pending_fetch_group(group_id));
  }

  #[test]
  fn size_playlist_continuation_keeps_fetch_group_until_last_child_exits() {
    let group_id = "fetch-size-continuation";
    let parent = expand_fetch_request(initial(group_id)).pop().unwrap();
    let continuation = FetchRequest::SizePlaylist {
      batch: parent.batch.clone(),
      playlist: serde_json::from_value(serde_json::json!({
        "id": "playlist",
        "entries": [
          {"videoUrl": "https://example.com/one", "index": 1},
          {"videoUrl": "https://example.com/two", "index": 2},
        ],
      }))
      .unwrap(),
      format: FormatOptions {
        track_type: TrackType::Both,
        abr: None,
        height: None,
        fps: None,
        audio_encoding: None,
        video_encoding: None,
        audio_track: None,
        video_track: None,
      },
    };
    drop(parent);
    assert!(has_pending_fetch_group(group_id));
    let mut children = expand_fetch_request(continuation);
    assert_eq!(children.len(), 2);
    drop(children.pop());
    assert!(has_pending_fetch_group(group_id));
    drop(children);
    assert!(!has_pending_fetch_group(group_id));
  }
}
