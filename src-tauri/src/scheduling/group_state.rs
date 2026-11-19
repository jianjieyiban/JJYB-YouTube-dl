use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use tempfile::TempDir;
use tokio::sync::{watch, Mutex, OwnedMutexGuard};

static FETCH_GROUPS: LazyLock<StdMutex<HashMap<String, FetchGroup>>> =
  LazyLock::new(|| StdMutex::new(HashMap::new()));

#[derive(Default)]
struct FetchGroup {
  current: Option<Arc<AtomicBool>>,
  batches: usize,
}

/// Shared by a submission and every entry or continuation derived from it.
pub struct FetchBatch {
  group_id: String,
  running: Arc<AtomicBool>,
}

impl FetchBatch {
  pub fn submit(group_id: &str) -> Arc<Self> {
    let mut groups = FETCH_GROUPS.lock().unwrap();
    let group = groups.entry(group_id.to_owned()).or_default();
    group.batches += 1;
    let running = group
      .current
      .get_or_insert_with(|| Arc::new(AtomicBool::new(true)))
      .clone();
    Arc::new(Self {
      group_id: group_id.to_owned(),
      running,
    })
  }

  pub fn group_id(&self) -> &String {
    &self.group_id
  }

  pub fn is_running(&self) -> bool {
    self.running.load(Ordering::Acquire)
  }
}

impl Drop for FetchBatch {
  fn drop(&mut self) {
    let mut groups = FETCH_GROUPS.lock().unwrap();
    let group = groups.get_mut(&self.group_id).unwrap();
    group.batches -= 1;
    if group.batches == 0 {
      groups.remove(&self.group_id);
    }
  }
}

pub fn cancel_fetch_group(group_id: &str) {
  let mut groups = FETCH_GROUPS.lock().unwrap();
  if let Some(running) = groups
    .get_mut(group_id)
    .and_then(|group| group.current.take())
  {
    running.store(false, Ordering::Release);
  }
}

#[cfg(test)]
pub(crate) fn has_pending_fetch_group(group_id: &str) -> bool {
  FETCH_GROUPS.lock().unwrap().contains_key(group_id)
}

#[cfg(windows)]
pub fn has_pending_groups() -> bool {
  !FETCH_GROUPS.lock().unwrap().is_empty()
    || DOWNLOAD_GROUPS
      .lock()
      .unwrap()
      .values()
      .any(|group| group.batches != 0)
}

// Download submissions must not share the fetch dispatcher's group-ID cleanup.
static DOWNLOAD_GROUPS: LazyLock<StdMutex<HashMap<String, DownloadGroup>>> =
  LazyLock::new(|| StdMutex::new(HashMap::new()));

#[derive(Default)]
struct DownloadGroup {
  current: Option<DownloadGeneration>,
  batches: usize,
  items: HashMap<String, Arc<DownloadSlot>>,
}

#[derive(Clone)]
struct DownloadGeneration {
  running: watch::Sender<bool>,
  paused: Arc<AtomicBool>,
  commit_gate: Arc<StdMutex<()>>,
}

#[derive(Clone, serde::Serialize)]
pub struct CompletedDownload {
  id: String,
  destination: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedDownload {
  id: String,
  message: String,
  published_destinations: Vec<String>,
}

#[derive(Clone)]
enum DownloadOutcome {
  Completed(CompletedDownload),
  Failed(FailedDownload),
}

#[derive(Default, serde::Serialize)]
pub struct DownloadCancelReceipt {
  completed: Vec<CompletedDownload>,
  failed: Vec<FailedDownload>,
}

impl DownloadCancelReceipt {
  fn push(&mut self, outcome: DownloadOutcome) {
    match outcome {
      DownloadOutcome::Completed(value) => self.completed.push(value),
      DownloadOutcome::Failed(value) => self.failed.push(value),
    }
  }
}

#[must_use]
#[derive(Default)]
pub struct PendingDownloadCancel {
  receipt: DownloadCancelReceipt,
  active: Vec<watch::Receiver<Option<DownloadOutcome>>>,
}

impl PendingDownloadCancel {
  pub async fn wait(mut self) -> DownloadCancelReceipt {
    for mut run in self.active {
      // The sender lives in DownloadRun and closes after staging cleanup and all events.
      while run.changed().await.is_ok() {}
      if let Some(outcome) = run.borrow().clone() {
        self.receipt.push(outcome);
      }
    }
    self
      .receipt
      .completed
      .sort_unstable_by(|a, b| a.id.cmp(&b.id));
    self.receipt.failed.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    self.receipt
  }
}

pub struct DownloadBatch {
  group_id: String,
  generation: DownloadGeneration,
  pub attempt_id: Option<String>,
}

impl DownloadBatch {
  #[cfg(test)]
  fn submit(group_id: &str) -> Arc<Self> {
    Self::submit_with_attempt(group_id, None)
  }

  pub fn submit_with_attempt(group_id: &str, attempt_id: Option<String>) -> Arc<Self> {
    let mut groups = DOWNLOAD_GROUPS.lock().unwrap();
    let group = groups.entry(group_id.to_owned()).or_default();
    group.batches += 1;
    let generation = group.current.get_or_insert_with(|| DownloadGeneration {
      running: watch::channel(true).0,
      paused: Arc::new(AtomicBool::new(false)),
      commit_gate: Arc::new(StdMutex::new(())),
    });
    Arc::new(Self {
      group_id: group_id.to_owned(),
      generation: generation.clone(),
      attempt_id,
    })
  }

  pub fn subscribe(&self) -> watch::Receiver<bool> {
    self.generation.running.subscribe()
  }

  pub fn slot(&self, id: &str) -> Arc<DownloadSlot> {
    DOWNLOAD_GROUPS
      .lock()
      .unwrap()
      .get_mut(&self.group_id)
      .unwrap()
      .items
      .entry(id.to_owned())
      .or_default()
      .clone()
  }
}

impl Drop for DownloadBatch {
  fn drop(&mut self) {
    let mut groups = DOWNLOAD_GROUPS.lock().unwrap();
    let group = groups.get_mut(&self.group_id).unwrap();
    group.batches -= 1;
    if group.batches == 0 {
      group.current = None;
      if !group.items.values().any(|slot| {
        let state = slot.state.lock().unwrap();
        state.staging.is_some() || state.outcome.is_some()
      }) {
        groups.remove(&self.group_id);
      }
    }
  }
}

pub fn cancel_download_group(group_id: &str, retain_partial: bool) -> PendingDownloadCancel {
  let mut groups = DOWNLOAD_GROUPS.lock().unwrap();
  let mut pending = PendingDownloadCancel::default();
  let Some(group) = groups.get_mut(group_id) else {
    return pending;
  };
  if let Some(generation) = group.current.take() {
    let _gate = generation.commit_gate.lock().unwrap();
    generation.paused.store(retain_partial, Ordering::Release);
    generation.running.send_replace(false);
  }
  for slot in group.items.values() {
    let mut state = slot.state.lock().unwrap();
    if let Some(active) = &state.active {
      pending.active.push(active.clone());
    } else if let Some(outcome) = &state.outcome {
      pending.receipt.push(outcome.clone());
    }
    if !retain_partial {
      state.numbering = None;
      state.outcome = None;
      if state.active.is_some() {
        state.discard = true;
      } else {
        state.staging = None;
      }
    }
  }
  if !retain_partial && group.batches == 0 {
    groups.remove(group_id);
  }
  pending
}

#[derive(Default)]
pub struct DownloadSlot {
  lock: Arc<Mutex<()>>,
  state: StdMutex<SlotState>,
}

#[derive(Default)]
struct SlotState {
  active: Option<watch::Receiver<Option<DownloadOutcome>>>,
  outcome: Option<DownloadOutcome>,
  discard: bool,
  numbering: Option<(u64, Option<u64>)>,
  staging: Option<(String, TempDir)>,
}

impl DownloadSlot {
  pub fn numbering(&self, proposed: (u64, Option<u64>)) -> (u64, Option<u64>) {
    *self.state.lock().unwrap().numbering.get_or_insert(proposed)
  }

  pub async fn begin(self: &Arc<Self>, batch: &DownloadBatch, id: &str) -> Option<DownloadRun> {
    let lock = self.lock.clone().lock_owned().await;
    let mut state = self.state.lock().unwrap();
    if !*batch.generation.running.borrow() {
      return None;
    }
    let (completion, receiver) = watch::channel(None);
    state.active = Some(receiver);
    state.outcome = None;
    Some(DownloadRun {
      id: id.to_owned(),
      slot: self.clone(),
      generation: batch.generation.clone(),
      completion,
      committing: false,
      published_destinations: Vec::new(),
      _lock: lock,
    })
  }

  pub fn staging_dir(&self, output_template: &str) -> std::io::Result<PathBuf> {
    let mut state = self.state.lock().unwrap();
    if state.discard {
      return Err(std::io::ErrorKind::Interrupted.into());
    }
    if let Some((template, dir)) = &state.staging {
      if template == output_template {
        return Ok(dir.path().to_owned());
      }
    }
    // Place staging on the destination volume. Ignore unresolved yt-dlp directory components.
    let parent = Path::new(output_template)
      .parent()
      .unwrap_or_else(|| Path::new(""));
    let base = parent
      .components()
      .take_while(|part| !part.as_os_str().to_string_lossy().contains("%("))
      .collect::<PathBuf>();
    let base = if base.as_os_str().is_empty() {
      Path::new(".")
    } else {
      &base
    };
    let base = std::path::absolute(base)?;
    std::fs::create_dir_all(&base)?;
    let dir = tempfile::Builder::new()
      .prefix(".ovd-download-")
      .tempdir_in(&base)?;
    let path = dir.path().to_owned();
    state.staging = Some((output_template.to_owned(), dir));
    Ok(path)
  }
}

pub struct DownloadRun {
  id: String,
  slot: Arc<DownloadSlot>,
  generation: DownloadGeneration,
  published_destinations: Vec<String>,
  completion: watch::Sender<Option<DownloadOutcome>>,
  committing: bool,
  _lock: OwnedMutexGuard<()>,
}

impl DownloadRun {
  /// One decision covers all output sets of this item, without holding a lock over filesystem I/O.
  pub fn try_commit(&mut self) -> bool {
    let _gate = self.generation.commit_gate.lock().unwrap();
    self.committing = *self.generation.running.borrow();
    self.committing
  }

  pub fn preserve_unconfirmed_staging(&mut self) {
    self.failed("Could not confirm yt-dlp exit; partial files were left on disk".into());
    let staging = self.slot.state.lock().unwrap().staging.take();
    if let Some((_, dir)) = staging {
      // Do not remove files an unconfirmed process may still be writing.
      let path = dir.keep();
      tracing::warn!(path = %path.display(), "Leaving unconfirmed yt-dlp staging on disk");
    }
  }

  pub fn completed(&self, destination: String) {
    debug_assert!(self.committing);
    self
      .completion
      .send_replace(Some(DownloadOutcome::Completed(CompletedDownload {
        id: self.id.clone(),
        destination,
      })));
  }

  pub fn published(&mut self, destination: String) {
    debug_assert!(self.committing);
    self.published_destinations.push(destination);
  }

  pub fn failed(&mut self, message: String) {
    self
      .completion
      .send_replace(Some(DownloadOutcome::Failed(FailedDownload {
        id: self.id.clone(),
        message,
        published_destinations: std::mem::take(&mut self.published_destinations),
      })));
  }
}

impl Drop for DownloadRun {
  fn drop(&mut self) {
    let mut state = self.slot.state.lock().unwrap();
    if state.discard || self.committing || !self.generation.paused.load(Ordering::Acquire) {
      state.staging = None;
    }
    if !state.discard {
      state.outcome = self.completion.borrow().clone();
    }
    state.active = None;
    state.discard = false;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn unconfirmed_exit_leaves_staging_intact_and_resume_uses_a_new_directory() {
    let id = "pause-unconfirmed-exit";
    let old = DownloadBatch::submit(id);
    let slot = old.slot("clip");
    let root = tempfile::tempdir().unwrap();
    let template = root.path().join("clip.%(ext)s");
    let template = template.to_str().unwrap();
    let mut running = slot.begin(&old, "clip").await.unwrap();
    let partial = slot.staging_dir(template).unwrap();
    std::fs::write(partial.join("clip.mp4.part"), b"partial").unwrap();

    let _ = cancel_download_group(id, true);
    running.preserve_unconfirmed_staging();
    drop(running);

    let resumed = DownloadBatch::submit(id);
    let slot = resumed.slot("clip");
    let _running = slot.begin(&resumed, "clip").await.unwrap();
    assert_ne!(slot.staging_dir(template).unwrap(), partial);
    assert_eq!(
      std::fs::read(partial.join("clip.mp4.part")).unwrap(),
      b"partial"
    );
  }

  #[tokio::test]
  async fn pause_resume_keeps_partial_without_reactivating_old_batch() {
    let id = "pause-resume-watch";
    let old = DownloadBatch::submit(id);
    let slot = old.slot("clip");
    let root = tempfile::tempdir().unwrap();
    let template = root.path().join("clip.%(ext)s");
    let template = template.to_str().unwrap();
    let running = slot.begin(&old, "clip").await.unwrap();
    assert_eq!(slot.numbering((1, Some(2))), (1, Some(2)));
    let partial = slot.staging_dir(template).unwrap();
    std::fs::write(partial.join("clip.mp4.part"), b"partial").unwrap();

    let _ = cancel_download_group(id, true);
    let resumed = DownloadBatch::submit(id);
    assert!(!*old.subscribe().borrow());
    assert!(*resumed.subscribe().borrow());
    assert!(slot.lock.try_lock().is_err());
    drop(running);
    drop(old);

    let slot = resumed.slot("clip");
    assert_eq!(slot.numbering((3, Some(4))), (1, Some(2)));
    let running = slot.begin(&resumed, "clip").await.unwrap();
    assert_eq!(slot.staging_dir(template).unwrap(), partial);
    assert!(partial.join("clip.mp4.part").is_file());
    let _ = cancel_download_group(id, false);
    assert!(partial.exists());
    drop(running);
    assert!(!partial.exists());
    drop(resumed);
    assert!(!DOWNLOAD_GROUPS.lock().unwrap().contains_key(id));
  }

  #[tokio::test]
  async fn cancellation_winner_cannot_start_publication() {
    let id = "cancel-before-commit";
    let batch = DownloadBatch::submit(id);
    let mut run = batch.slot("clip").begin(&batch, "clip").await.unwrap();
    let ack = cancel_download_group(id, false);
    assert!(!run.try_commit());
    drop(run);
    assert!(ack.wait().await.completed.is_empty());
  }

  #[tokio::test]
  async fn publication_winner_is_returned_only_after_the_run_finishes() {
    let id = "cancel-after-commit";
    let batch = DownloadBatch::submit(id);
    let mut run = batch.slot("clip").begin(&batch, "clip").await.unwrap();
    assert!(run.try_commit());
    let mut ack = std::pin::pin!(cancel_download_group(id, true).wait());
    assert!(futures::poll!(ack.as_mut()).is_pending());
    run.completed("published.mp4".into());
    // Completion is recorded before emitting the event; the receipt still waits.
    assert!(futures::poll!(ack.as_mut()).is_pending());
    drop(run);
    let completed = ack.await.completed;
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].id, "clip");
    assert_eq!(completed[0].destination, "published.mp4");
  }

  #[tokio::test]
  async fn cancellation_receipt_keeps_partial_publication_as_failure() {
    let id = "cancel-after-partial-publication";
    let batch = DownloadBatch::submit(id);
    let mut run = batch.slot("clip").begin(&batch, "clip").await.unwrap();
    assert!(run.try_commit());
    let output = tempfile::NamedTempFile::new().unwrap();
    let destination = output.path().to_string_lossy().into_owned();
    run.published(destination.clone());
    let ack = cancel_download_group(id, false);
    run.failed("second output failed".into());
    drop(run);
    let receipt = ack.wait().await;
    assert!(receipt.completed.is_empty());
    assert_eq!(receipt.failed.len(), 1);
    assert_eq!(receipt.failed[0].published_destinations, [destination]);
    assert!(output.path().is_file());
  }

  #[tokio::test]
  async fn cancellation_receipt_includes_a_completion_from_a_drained_batch() {
    let id = "cancel-after-batch-drained";
    let batch = DownloadBatch::submit(id);
    let mut run = batch.slot("clip").begin(&batch, "clip").await.unwrap();
    assert!(run.try_commit());
    run.completed("published.mp4".into());
    drop(run);
    drop(batch);
    assert_eq!(
      cancel_download_group(id, false)
        .wait()
        .await
        .completed
        .len(),
      1
    );
    assert!(!DOWNLOAD_GROUPS.lock().unwrap().contains_key(id));
  }

  #[tokio::test]
  async fn deleting_an_inactive_paused_group_removes_partial() {
    let id = "delete-paused-download";
    let batch = DownloadBatch::submit(id);
    let slot = batch.slot("clip");
    let root = tempfile::tempdir().unwrap();
    let template = root.path().join("clip.%(ext)s");
    let running = slot.begin(&batch, "clip").await.unwrap();
    let partial = slot.staging_dir(template.to_str().unwrap()).unwrap();

    let _ = cancel_download_group(id, true);
    drop(running);
    drop(batch);
    assert!(partial.exists());
    let _ = cancel_download_group(id, false);
    assert!(!partial.exists());
    assert!(!DOWNLOAD_GROUPS.lock().unwrap().contains_key(id));
  }
}
