use crate::models::download::{DownloadOverrides, FormatOptions};
use crate::models::{DownloadItem, SubtitleInventory};
use crate::runners::template_context::TemplateContext;
use crate::runners::ytdlp_download::{run_ytdlp_download, YtdlpDownloadError};
use crate::scheduling::concurrency::DynamicSemaphore;
use crate::scheduling::dispatcher::{DispatchEntry, DispatchRequest, GenericDispatcher};
use crate::scheduling::group_state::{DownloadBatch, DownloadSlot};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::{mpsc::UnboundedSender, watch};

#[derive(Clone)]
pub struct DownloadSender(pub UnboundedSender<DispatchRequest<DownloadRequest>>);

#[derive(Clone)]
pub enum DownloadRequest {
  Batch {
    group_id: String,
    items: Vec<DownloadItem>,
    batch: Arc<DownloadBatch>,
  },
}

#[derive(Clone)]
pub struct DownloadEntry {
  pub group_id: String,
  pub id: String,
  pub url: String,
  pub format: FormatOptions,
  pub subtitles_only: bool,
  pub thumbnail_only: bool,
  pub subtitle_inventory: Option<SubtitleInventory>,
  pub overrides: Option<DownloadOverrides>,
  pub template_context: TemplateContext,
  pub running: watch::Receiver<bool>,
  batch: Arc<DownloadBatch>,
  slot: Arc<DownloadSlot>,
}

impl DownloadEntry {
  pub fn attempt_id(&self) -> Option<&str> {
    self.batch.attempt_id.as_deref()
  }

  fn new(item: DownloadItem, group_id: String, batch: Arc<DownloadBatch>) -> Self {
    let slot = batch.slot(&item.id);
    Self {
      group_id,
      id: item.id,
      url: item.url,
      format: item.format,
      subtitles_only: item.subtitles_only,
      thumbnail_only: item.thumbnail_only,
      subtitle_inventory: item.subtitle_inventory,
      overrides: item.overrides,
      template_context: item.template_context,
      running: batch.subscribe(),
      batch,
      slot,
    }
  }

  pub fn staging_dir(&self, output_template: &str) -> std::io::Result<PathBuf> {
    self.slot.staging_dir(output_template)
  }
}

impl DispatchEntry for DownloadEntry {
  fn group_id(&self) -> &String {
    &self.group_id
  }
  fn is_running(&self) -> bool {
    *self.running.borrow()
  }
  fn group_key(&self) -> Option<&String> {
    self.template_context.values.get("playlist_id")
  }
  fn set_numbering(&mut self, autonumber: u64, group_autonumber: Option<u64>) {
    let (autonumber, group_autonumber) = self.slot.numbering((autonumber, group_autonumber));
    self
      .template_context
      .values
      .insert("autonumber".to_string(), autonumber.to_string());
    if let Some(group_autonumber) = group_autonumber {
      self.template_context.values.insert(
        "playlist_autonumber".to_string(),
        group_autonumber.to_string(),
      );
    }
  }
}

pub fn setup_download_dispatcher(
  app: &AppHandle,
  sem: Arc<DynamicSemaphore>,
) -> GenericDispatcher<DownloadRequest> {
  GenericDispatcher::start(
    app.clone(),
    sem,
    |req: DownloadRequest| match req {
      DownloadRequest::Batch {
        group_id,
        items,
        batch,
      } => items
        .into_iter()
        .map(|item| DownloadEntry::new(item, group_id.clone(), batch.clone()))
        .collect(),
    },
    |_tx, app: AppHandle, entry: DownloadEntry| async move {
      let Some(mut run) = entry.slot.begin(&entry.batch, &entry.id).await else {
        return;
      };
      tracing::info!(download_id = %entry.id, "Starting download");

      if let Err(e) = run_ytdlp_download(app, entry.clone(), &mut run).await {
        run.failed(e.to_string());
        tracing::warn!(
          download_id = %entry.id,
          group_id = %entry.group_id,
          error = %e,
          "Failed to run ytdlp download",
        );
        if should_report_to_sentry(&e) {
          sentry::capture_error(&e);
        }
      }
    },
  )
}

fn should_report_to_sentry(err: &YtdlpDownloadError) -> bool {
  matches!(
    err,
    YtdlpDownloadError::SpawnFailed(_)
      | YtdlpDownloadError::InvalidDiagnosticRules(_)
      | YtdlpDownloadError::EventStreamEnded
  )
}
