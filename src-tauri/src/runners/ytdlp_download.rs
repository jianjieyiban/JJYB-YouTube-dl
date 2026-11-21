use crate::logging::LogStoreState;
use crate::models::download::DownloadSection;
use crate::models::{
  MediaDiagnosticPayload, MediaFatalPayload, MediaProgressComplete, ProgressEvent,
};
use crate::parsers::ytdlp_error::{DiagnosticMatcher, YtdlpErrorParser};
use crate::parsers::ytdlp_progress::{progress_category_for_track_type, YtdlpProgressParser};
use crate::runners::ytdlp_runner::{YtdlpCommandEvent, YtdlpRunner};
use crate::scheduling::download_pipeline::DownloadEntry;
use crate::scheduling::group_state::DownloadRun;
use std::{
  fmt, fs, io,
  path::{Component, Path, PathBuf},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::watch;

#[derive(Debug)]
pub enum YtdlpDownloadError {
  InvalidArguments(String),
  InvalidDiagnosticRules(String),
  SpawnFailed(String),
  RunnerError(String),
  NonZeroExit(i32),
  EventStreamEnded,
}

impl fmt::Display for YtdlpDownloadError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::InvalidArguments(e) => write!(f, "Invalid yt-dlp arguments: {e}"),
      Self::InvalidDiagnosticRules(e) => write!(f, "Invalid diagnostic rules: {e}"),
      Self::SpawnFailed(e) => write!(f, "Failed to spawn yt-dlp: {e}"),
      Self::RunnerError(e) => write!(f, "yt-dlp runner error: {e}"),
      Self::NonZeroExit(code) => write!(f, "yt-dlp exited with code {code}"),
      Self::EventStreamEnded => write!(f, "yt-dlp event stream ended unexpectedly"),
    }
  }
}

impl std::error::Error for YtdlpDownloadError {}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadEvent<'a, T> {
  #[serde(skip_serializing_if = "Option::is_none")]
  attempt_id: Option<&'a str>,
  #[serde(flatten)]
  payload: T,
}

fn emit_download<T: Clone + serde::Serialize>(
  app: &AppHandle,
  entry: &DownloadEntry,
  name: &str,
  payload: T,
) {
  let _ = app.emit(
    name,
    DownloadEvent {
      attempt_id: entry.attempt_id(),
      payload,
    },
  );
}

pub async fn run_ytdlp_download(
  app: AppHandle,
  entry: DownloadEntry,
  run: &mut DownloadRun,
) -> Result<(), YtdlpDownloadError> {
  let tmp_dir = app.state::<crate::paths::PathsManager>().tmp_dir();
  fs::create_dir_all(&tmp_dir).map_err(|e| YtdlpDownloadError::InvalidArguments(e.to_string()))?;
  let manifest = tempfile::NamedTempFile::new_in(&tmp_dir)
    .map_err(|e| YtdlpDownloadError::InvalidArguments(e.to_string()))?;
  let media_manifest = tempfile::NamedTempFile::new_in(&tmp_dir)
    .map_err(|e| YtdlpDownloadError::InvalidArguments(e.to_string()))?;
  let config = app.state::<crate::SharedConfig>().load();
  let subtitle_settings: crate::state::config_models::SubtitleSettings =
    crate::runners::override_resolver::resolve_with_patch(
      &config.subtitles,
      entry
        .overrides
        .as_ref()
        .and_then(|value| value.subtitles.as_ref()),
    );
  let runner = YtdlpRunner::new(&app)
    .with_progress_args()
    .with_network_args(entry.overrides.as_ref())
    .with_auth_args(entry.overrides.as_ref())
    .with_input_filter_args(entry.overrides.as_ref());
  let (runner, requested_subtitles) = if entry.thumbnail_only {
    (runner.with_args(thumbnail_only_args()), false)
  } else {
    runner.with_subtitle_args(entry.overrides.as_ref(), entry.subtitle_inventory.as_ref())
  };
  if !entry.thumbnail_only && entry.subtitles_only && !requested_subtitles {
    if !is_cancelled_now(&entry.running) {
      return Err(subtitle_failure(
        &app,
        &entry,
        super::subtitles::SubtitleError::NoSubtitles,
      ));
    }
    return Ok(());
  }
  let runner = if entry.thumbnail_only {
    runner
  } else if entry.subtitles_only {
    runner.with_args(["--skip-download"])
  } else {
    runner
      .with_sponsorblock_args(entry.overrides.as_ref())
      .with_format_args(&entry.format, entry.overrides.as_ref())
      .with_input_args(entry.overrides.as_ref())
  };
  let output_args = match if entry.subtitles_only || entry.thumbnail_only {
    Ok(Vec::new())
  } else {
    runner.output_args(&entry.format, entry.overrides.as_ref())
  } {
    Ok(args) => args,
    Err(err) => {
      emit_internal_fatal(
        &app,
        &entry,
        format!("Invalid download configuration: {err}"),
        Some(err.clone()),
      );
      return Err(YtdlpDownloadError::InvalidArguments(err));
    }
  };
  let template = runner.output_template(
    &entry.format.track_type,
    &entry.template_context,
    entry.overrides.as_ref(),
  );
  let mut cancel_rx = entry.running.clone();
  if is_cancelled_now(&cancel_rx) {
    return Ok(());
  }
  let (target, relative_template) =
    split_output_template(&template).map_err(|err| output_failure(&app, &entry, err))?;
  let stage = match entry.staging_dir(&template.to_string_lossy()) {
    Ok(path) => path,
    Err(_) if is_cancelled_now(&cancel_rx) => return Ok(()),
    Err(err) => return Err(output_failure(&app, &entry, err)),
  };
  let thumbnails =
    tempfile::NamedTempFile::new_in(&tmp_dir).map_err(|err| output_failure(&app, &entry, err))?;
  let save_thumbnail =
    entry.thumbnail_only || output_args.iter().any(|arg| arg == "--write-thumbnail");
  let runner = runner
    .with_args(output_args)
    .with_args(["-o".to_owned(), staged_template(&stage, &relative_template)])
    .with_args([
      "--print-to-file".to_owned(),
      "after_video:%(thumbnails)j".into(),
      thumbnails.path().to_string_lossy().into_owned(),
    ])
    .with_args([
      "--no-simulate".to_string(),
      "--print-to-file".into(),
      if entry.subtitles_only {
        "before_dl:%(requested_subtitles)j".into()
      } else {
        "after_move:%(requested_subtitles)j".into()
      },
      manifest.path().to_string_lossy().into_owned(),
      "--print-to-file".into(),
      "after_move:%(filepath)j".into(),
      media_manifest.path().to_string_lossy().into_owned(),
    ])
    .with_url(&entry.url);

  static RULES_JSON: &str = include_str!("../diagnostic_rules.json");
  let matcher = match DiagnosticMatcher::from_json(RULES_JSON) {
    Ok(matcher) => matcher,
    Err(err) => {
      emit_internal_fatal(
        &app,
        &entry,
        format!("Invalid diagnostic rules: {err}"),
        Some(err.to_string()),
      );
      return Err(YtdlpDownloadError::InvalidDiagnosticRules(err.to_string()));
    }
  };

  let error_parser = YtdlpErrorParser::new(&entry.id, &entry.group_id, matcher);
  let partial_download_duration_secs = entry
    .overrides
    .as_ref()
    .and_then(|overrides| overrides.output.as_ref())
    .and_then(|output| output.partial_download.as_ref())
    .and_then(|partial| partial.section.as_ref())
    .and_then(download_section_duration_secs)
    .filter(|value| *value > 0.0);
  let mut progress_parser = YtdlpProgressParser::new(
    &entry.id,
    &entry.group_id,
    progress_category_for_track_type(&entry.format.track_type),
    partial_download_duration_secs,
  );

  let (mut rx, mut child) = match runner.spawn() {
    Ok(result) => result,
    Err(err) => {
      let message = format!("Failed to spawn yt-dlp: {err}");
      emit_download(
        &app,
        &entry,
        "media_fatal",
        MediaFatalPayload::with_exit(entry.group_id.clone(), entry.id.clone(), 1, message),
      );
      return Err(YtdlpDownloadError::SpawnFailed(err));
    }
  };

  loop {
    tokio::select! {
      event = rx.recv() => {
        let Some(event) = event else {
          if child.shutdown().await.is_err() {
            run.preserve_unconfirmed_staging();
          }
          break;
        };

        if is_cancelled_now(&cancel_rx) {
          if !matches!(event, YtdlpCommandEvent::Terminated(_)) && child.shutdown().await.is_err() {
            run.preserve_unconfirmed_staging();
          }
          return Ok(());
        }

        let log_state = app.state::<LogStoreState>();

        match event {
          YtdlpCommandEvent::Stdout(line) => {
            let line_str = String::from_utf8_lossy(&line);
            store_log_line(&line_str, &entry, log_state, &app);
            parse_progress_line(&line_str, &mut progress_parser, &app, &entry);
            parse_error_line(&line_str, &error_parser, &app, &entry);
          }
          YtdlpCommandEvent::Stderr(line) => {
            let line_str = String::from_utf8_lossy(&line);
            store_log_line(&line_str, &entry, log_state, &app);
            parse_progress_line(&line_str, &mut progress_parser, &app, &entry);
            parse_error_line(&line_str, &error_parser, &app, &entry);
          }
          YtdlpCommandEvent::Terminated(term) => {
            if term.code == Some(0) {
              let mut covers = if save_thumbnail {
                thumbnail_artifacts(thumbnails.path()).map_err(|err| output_failure(&app, &entry, err))?
              } else {
                Vec::new()
              };
              let explicit_tracks = entry.overrides.as_ref().and_then(|value| value.subtitles.as_ref()).is_some_and(|value| value.manual_languages.is_some() || value.auto_languages.is_some());
              let subtitles = finish_download_subtitles(manifest.path(), &subtitle_settings, requested_subtitles, explicit_tracks, entry.subtitles_only)
                .map_err(|err| subtitle_failure(&app, &entry, err))?;
              let primary_files = if entry.thumbnail_only {
                std::mem::take(&mut covers)
              } else if entry.subtitles_only {
                subtitles.clone()
              } else {
                media_artifacts(media_manifest.path()).map_err(|err| output_failure(&app, &entry, err))?
              };
              let sets = group_artifacts(primary_files, subtitles, covers, entry.subtitles_only, &stage)
                .map_err(|err| output_failure(&app, &entry, err))?;
              for set in &sets {
                if is_cancelled_now(&cancel_rx) { return Ok(()); }
                if !entry.subtitles_only && subtitle_settings.embed_subtitles && !set.subtitles.is_empty() {
                  if let Err(err) = embed_subtitles(&app, &set.primary, &set.subtitles, &mut cancel_rx).await {
                    if is_cancelled_now(&cancel_rx) { return Ok(()); }
                    emit_embed_warning(&app, &entry, err);
                  }
                }
              }
              if !run.try_commit() { return Ok(()); }
              let mut destination = String::new();
              for set in sets {
                let sidecars = [set.subtitles, set.covers].concat();
                let output = publish_artifact_set(&target, &stage, &set.primary, &set.stem, &sidecars)
                  .map_err(|err| output_failure(&app, &entry, err))?;
                destination = output.to_string_lossy().into_owned();
                run.published(destination.clone());
                emit_download(&app, &entry, "media_destination", crate::models::MediaDestination {
                  id: entry.id.clone(),
                  group_id: entry.group_id.clone(),
                  destination: crate::models::progress::MediaDestinationPath {
                    confidence: 100,
                    path: output.to_string_lossy().into_owned(),
                  },
                  is_merged: false,
                });
              }
              run.completed(destination);
              emit_download(&app, &entry,
                "media_complete",
                MediaProgressComplete {
                  id: entry.id.clone(),
                  group_id: entry.group_id.clone(),
                },
              );
              return Ok(());
            }

            let exit = term.code.unwrap_or(1);
            emit_download(&app, &entry,
              "media_fatal",
              MediaFatalPayload::with_exit(
                entry.group_id.clone(),
                entry.id.clone(),
                exit,
                format!("Download failed for group {}", entry.group_id),
              ),
            );
            return Err(YtdlpDownloadError::NonZeroExit(exit));
          }
          YtdlpCommandEvent::Error(err) => {
            if child.shutdown().await.is_err() {
              run.preserve_unconfirmed_staging();
            }
            let msg = format!("Download failed for group {}: {}", entry.group_id, err);
            emit_internal_fatal(&app, &entry, msg.clone(), Some(err.clone()));

            return Err(YtdlpDownloadError::RunnerError(err));
          }
        }
      }
      _ = cancel_rx.changed() => {
        if is_cancelled_now(&cancel_rx) {
          if child.shutdown().await.is_err() {
            run.preserve_unconfirmed_staging();
          }
          return Ok(());
        }
      }
    }
  }

  emit_internal_fatal(
    &app,
    &entry,
    "yt-dlp event stream ended unexpectedly".to_string(),
    None,
  );
  Err(YtdlpDownloadError::EventStreamEnded)
}

/// Keep literal path components on the destination volume; let yt-dlp render
/// any remaining dynamic directory components inside the job's staging area.
fn split_output_template(template: &Path) -> io::Result<(PathBuf, PathBuf)> {
  let mut root = PathBuf::new();
  let mut relative = PathBuf::new();
  let mut components = template.components().peekable();
  while let Some(component) = components.next() {
    let part = component.as_os_str();
    if components.peek().is_none()
      || !relative.as_os_str().is_empty()
      || part.to_string_lossy().contains("%(")
    {
      if !matches!(component, Component::Normal(_)) {
        return Err(io::Error::other("Invalid relative output template"));
      }
      relative.push(part);
    } else {
      root.push(part);
    }
  }
  if relative.as_os_str().is_empty() {
    return Err(io::Error::other("Missing output filename template"));
  }
  if root.as_os_str().is_empty() {
    root.push(".");
  }
  Ok((std::path::absolute(root)?, relative))
}

fn checked_artifact(stage: &Path, file: &Path) -> io::Result<PathBuf> {
  let local = file.strip_prefix(stage).map_err(io::Error::other)?;
  if !local
    .components()
    .all(|part| matches!(part, Component::Normal(_)))
    || !fs::symlink_metadata(file)?.is_file()
    || !fs::canonicalize(file)?.starts_with(fs::canonicalize(stage)?)
  {
    return Err(io::Error::other("Invalid staged output file"));
  }
  Ok(local.to_path_buf())
}

fn publish_artifact_set(
  target: &Path,
  stage: &Path,
  primary: &Path,
  stem: &str,
  sidecars: &[PathBuf],
) -> io::Result<PathBuf> {
  let mut files = Vec::new();
  for source in std::iter::once(primary).chain(sidecars.iter().map(PathBuf::as_path)) {
    if files.iter().any(|(known, _, _)| *known == source) {
      continue;
    }
    let relative = checked_artifact(stage, source)?;
    let name = relative
      .file_name()
      .and_then(|value| value.to_str())
      .ok_or_else(|| io::Error::other("Missing output filename"))?;
    let suffix = name
      .strip_prefix(stem)
      .filter(|value| value.starts_with('.') || value.is_empty())
      .ok_or_else(|| io::Error::other("Output artifact does not match the media name"))?;
    let directory = target.join(relative.parent().unwrap_or(Path::new("")));
    fs::create_dir_all(&directory)?;
    files.push((source, directory, suffix.to_owned()));
  }

  // Keep an open identity for rollback: a different file at the same path
  // must never be deleted. Publish the primary file first as the common claim.
  'number: for number in 1..=10_000 {
    let base = if number == 1 {
      stem.to_owned()
    } else {
      format!("{stem} ({number})")
    };
    let mut published = Vec::new();
    for (source, directory, suffix) in &files {
      let dest = directory.join(format!("{base}{suffix}"));
      let result = same_file::Handle::from_path(source).and_then(|identity| {
        claim_output(source, &dest)?;
        Ok(identity)
      });
      match result {
        Ok(identity) => published.push((dest, identity, *source)),
        Err(err) => {
          for (path, identity, source) in published.into_iter().rev() {
            if same_file::Handle::from_path(&path).is_ok_and(|current| current == identity) {
              rollback_output(&path, source)?;
            }
          }
          if err.kind() == io::ErrorKind::AlreadyExists {
            continue 'number;
          }
          return Err(err);
        }
      }
    }
    return Ok(published.remove(0).0);
  }
  Err(io::Error::new(
    io::ErrorKind::AlreadyExists,
    "No free numbered output filename",
  ))
}

#[cfg(windows)]
fn claim_output(source: &Path, destination: &Path) -> io::Result<()> {
  use std::os::windows::ffi::OsStrExt;
  use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
  let wide = |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain(Some(0)).collect() };
  let source = wide(source);
  let destination = wide(destination);
  // Without MOVEFILE_REPLACE_EXISTING, the filesystem rejects collisions.
  if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
    return Err(io::Error::last_os_error());
  }
  Ok(())
}

#[cfg(not(windows))]
fn claim_output(source: &Path, destination: &Path) -> io::Result<()> {
  fs::hard_link(source, destination)
}

#[cfg(windows)]
fn rollback_output(path: &Path, source: &Path) -> io::Result<()> {
  claim_output(path, source)
}

#[cfg(not(windows))]
fn rollback_output(path: &Path, _source: &Path) -> io::Result<()> {
  fs::remove_file(path)
}

fn thumbnail_only_args() -> Vec<&'static str> {
  vec![
    "--skip-download",
    "--write-thumbnail",
    "--convert-thumbnails",
    "jpg",
    "--no-write-subs",
    "--no-write-auto-subs",
    "--no-embed-subs",
    "--no-embed-thumbnail",
    "--no-embed-metadata",
  ]
}

fn staged_template(stage: &Path, relative: &Path) -> String {
  // A percent in the destination directory is literal, not a yt-dlp field.
  format!(
    "{}{sep}{}",
    stage.to_string_lossy().replace('%', "%%"),
    relative.display(),
    sep = std::path::MAIN_SEPARATOR
  )
}

fn media_artifacts(manifest: &Path) -> io::Result<Vec<PathBuf>> {
  fs::read_to_string(manifest)?
    .lines()
    .filter(|line| !line.trim().is_empty())
    .map(|line| serde_json::from_str::<PathBuf>(line).map_err(io::Error::other))
    .collect()
}

fn thumbnail_artifacts(manifest: &Path) -> io::Result<Vec<PathBuf>> {
  use std::io::Read;
  let mut files = Vec::new();
  for line in fs::read_to_string(manifest)?
    .lines()
    .filter(|line| !line.trim().is_empty())
  {
    let thumbnails: serde_json::Value = serde_json::from_str(line).map_err(io::Error::other)?;
    if let Some(thumbnails) = thumbnails.as_array() {
      for thumbnail in thumbnails.iter().rev() {
        let Some(path) = thumbnail.get("filepath").and_then(|value| value.as_str()) else {
          continue;
        };
        let path = PathBuf::from(path);
        let mut header = [0; 3];
        if path
          .extension()
          .is_some_and(|ext| ext.eq_ignore_ascii_case("jpg"))
          && fs::File::open(&path)
            .and_then(|mut file| file.read_exact(&mut header))
            .is_ok()
          && header == [0xff, 0xd8, 0xff]
        {
          files.push(path);
          break;
        }
      }
    }
  }
  Ok(files)
}

struct ArtifactSet {
  primary: PathBuf,
  stem: String,
  subtitles: Vec<PathBuf>,
  covers: Vec<PathBuf>,
}

fn group_artifacts(
  primary_files: Vec<PathBuf>,
  subtitles: Vec<PathBuf>,
  covers: Vec<PathBuf>,
  subtitles_only: bool,
  stage: &Path,
) -> io::Result<Vec<ArtifactSet>> {
  let mut sets = Vec::<ArtifactSet>::new();
  for primary in primary_files {
    checked_artifact(stage, &primary)?;
    let base = primary
      .file_stem()
      .and_then(|value| value.to_str())
      .ok_or_else(|| io::Error::other("Missing output filename"))?;
    let stem = if subtitles_only {
      base.rsplit_once('.').map_or(base, |(stem, _)| stem)
    } else {
      base
    };
    if sets.iter().any(|set| {
      set.primary == primary
        || (subtitles_only && set.stem == stem && set.primary.parent() == primary.parent())
    }) {
      continue;
    }
    let stem = stem.to_owned();
    sets.push(ArtifactSet {
      primary,
      stem,
      subtitles: Vec::new(),
      covers: Vec::new(),
    });
  }
  if sets.is_empty() {
    return Err(io::Error::other("No completed output file was recorded"));
  }
  for (path, subtitle) in subtitles
    .into_iter()
    .map(|p| (p, true))
    .chain(covers.into_iter().map(|p| (p, false)))
  {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
      continue;
    };
    let associated = sets
      .iter_mut()
      .filter(|set| {
        set.primary.parent() == path.parent()
          && name
            .strip_prefix(&set.stem)
            .is_some_and(|suffix| suffix.starts_with('.'))
      })
      .max_by_key(|set| set.stem.len());
    if let Some(set) = associated {
      if subtitle {
        set.subtitles.push(path);
      } else {
        set.covers.push(path);
      }
    }
  }
  Ok(sets)
}

fn output_failure(app: &AppHandle, entry: &DownloadEntry, err: io::Error) -> YtdlpDownloadError {
  let detail = err.to_string();
  let mut payload = MediaFatalPayload::internal(
    entry.group_id.clone(),
    entry.id.clone(),
    format!("Output publication failed: {detail}"),
    Some(detail.clone()),
  );
  if err.kind() == io::ErrorKind::AlreadyExists {
    payload.code = Some("outputPathConflict".into());
  }
  emit_download(app, entry, "media_fatal", payload);
  YtdlpDownloadError::RunnerError(detail)
}

fn finish_download_subtitles(
  manifest: &std::path::Path,
  settings: &crate::state::config_models::SubtitleSettings,
  requested_subtitles: bool,
  explicit_tracks: bool,
  subtitles_only: bool,
) -> Result<Vec<std::path::PathBuf>, super::subtitles::SubtitleError> {
  if !requested_subtitles {
    return Ok(Vec::new());
  }
  let required = subtitles_only || settings.embed_subtitles || explicit_tracks;
  // No selected tracks can be optional; a selected track with a missing or
  // invalid output is still a real processing failure.
  match super::subtitles::finish_subtitles(
    manifest,
    settings.chinese_priority && !explicit_tracks,
    true,
  ) {
    Err(super::subtitles::SubtitleError::NoSubtitles) if !required => Ok(Vec::new()),
    result => result,
  }
}

fn subtitle_failure(
  app: &AppHandle,
  entry: &DownloadEntry,
  err: super::subtitles::SubtitleError,
) -> YtdlpDownloadError {
  emit_download(
    app,
    entry,
    "media_fatal",
    subtitle_fatal(entry.group_id.clone(), entry.id.clone(), &err),
  );
  YtdlpDownloadError::RunnerError(err.to_string())
}

fn emit_embed_warning(app: &AppHandle, entry: &DownloadEntry, err: String) {
  emit_download(
    app,
    entry,
    "media_diagnostic",
    MediaDiagnosticPayload {
      id: entry.id.clone(),
      group_id: entry.group_id.clone(),
      level: crate::models::error::DiagnosticLevel::Warning,
      code: "subtitleEmbedFailed".into(),
      component: Some("ffmpeg".into()),
      message: format!(
        "Subtitle embedding failed; keeping the original media and separate SRT files: {err}"
      ),
      raw: err,
      timestamp: std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis(),
    },
  );
}

fn subtitle_fatal(
  group_id: String,
  id: String,
  error: &super::subtitles::SubtitleError,
) -> MediaFatalPayload {
  let mut payload = MediaFatalPayload::internal(
    group_id,
    id,
    format!("Subtitle processing failed: {error}"),
    Some(error.to_string()),
  );
  if matches!(error, super::subtitles::SubtitleError::NoSubtitles) {
    payload.code = Some("noSubtitles".into());
    payload.message = "找不到可下载的字幕，无法下载".into();
    payload.details = None;
  }
  payload
}

async fn embed_subtitles(
  app: &AppHandle,
  media: &Path,
  subtitles: &[PathBuf],
  cancel: &mut watch::Receiver<bool>,
) -> Result<(), String> {
  let resolver =
    crate::binaries::tool_resolver::ToolResolver::new(&app.state::<crate::paths::PathsManager>());
  let binary = tokio::task::spawn_blocking(move || resolver.ffmpeg_pair().0)
    .await
    .map_err(|e| e.to_string())?
    .ok_or("No usable ffmpeg found")?;
  super::subtitle_embed::embed_with_binary(&binary, media, subtitles, cancel).await
}

fn emit_internal_fatal(
  app: &AppHandle,
  entry: &DownloadEntry,
  message: String,
  details: Option<String>,
) {
  emit_download(
    app,
    entry,
    "media_fatal",
    MediaFatalPayload::internal(entry.group_id.clone(), entry.id.clone(), message, details),
  );
}

fn is_cancelled_now(cancel_rx: &watch::Receiver<bool>) -> bool {
  !*cancel_rx.borrow()
}

fn store_log_line(
  line: &str,
  entry: &DownloadEntry,
  log_state: State<LogStoreState>,
  app: &AppHandle,
) {
  if line.is_empty() || line.starts_with("RAW") {
    return;
  }
  let mut store = log_state.write();
  store.append_line(app, &entry.group_id, line);
}

fn parse_progress_line(
  line: &str,
  progress_parser: &mut YtdlpProgressParser,
  app: &AppHandle,
  entry: &DownloadEntry,
) {
  let progress_events = progress_parser.parse_line(line);

  for progress_event in progress_events {
    match progress_event {
      // Only the published path is safe to expose as the final destination.
      ProgressEvent::Destination(_) => {}
      ProgressEvent::Progress(progress) => {
        emit_download(app, entry, "media_progress", progress);
      }
      ProgressEvent::StageChange(progress) => {
        emit_download(app, entry, "media_progress_stage", progress);
      }
    }
  }
}

fn parse_error_line(
  line: &str,
  error_parser: &YtdlpErrorParser,
  app: &AppHandle,
  entry: &DownloadEntry,
) {
  if let Some(event) = error_parser.parse_line(line) {
    emit_download(
      app,
      entry,
      "media_diagnostic",
      MediaDiagnosticPayload::from_diagnostic_event(event),
    );
  }
}

fn download_section_duration_secs(section: &DownloadSection) -> Option<f64> {
  let start = parse_clock_secs(&section.start)?;
  let end = parse_clock_secs(&section.end)?;
  (end > start).then_some(end - start)
}

fn parse_clock_secs(value: &str) -> Option<f64> {
  let mut total = 0.0;
  let parts = value.trim().split(':').collect::<Vec<_>>();
  if parts.len() != 3 {
    return None;
  }

  for (index, part) in parts.iter().enumerate() {
    let component = part.trim().parse::<f64>().ok()?;
    total += component
      * match index {
        0 => 3600.0,
        1 => 60.0,
        2 => 1.0,
        _ => unreachable!(),
      };
  }

  Some(total)
}

#[cfg(test)]
mod tests {
  use super::finish_download_subtitles;
  use crate::state::config_models::SubtitleSettings;

  #[test]
  fn publication_keeps_existing_files_and_numbers_the_whole_set() {
    let target = tempfile::tempdir().unwrap();
    let staged = tempfile::tempdir_in(target.path()).unwrap();
    let video = staged.path().join("clip.v1.mp4");
    let subtitle = staged.path().join("clip.v1.en.srt");
    let cover = staged.path().join("clip.v1.jpg");
    std::fs::write(&video, b"new video").unwrap();
    std::fs::write(&subtitle, b"new captions").unwrap();
    std::fs::write(&cover, b"new cover").unwrap();
    std::fs::write(target.path().join("clip.v1.mp4"), b"old video").unwrap();
    std::fs::write(target.path().join("clip.v1 (2).en.srt"), b"old captions").unwrap();

    let output = super::publish_artifact_set(
      target.path(),
      staged.path(),
      &video,
      "clip.v1",
      &[subtitle, cover],
    )
    .unwrap();
    assert_eq!(output, target.path().join("clip.v1 (3).mp4"));
    for (name, content) in [
      ("clip.v1.mp4", "old video"),
      ("clip.v1 (2).en.srt", "old captions"),
      ("clip.v1 (3).mp4", "new video"),
      ("clip.v1 (3).en.srt", "new captions"),
      ("clip.v1 (3).jpg", "new cover"),
    ] {
      assert_eq!(
        std::fs::read_to_string(target.path().join(name)).unwrap(),
        content
      );
    }
    assert!(!target.path().join("clip.v1 (2).jpg").exists());
    assert!(!target.path().join("clip.v1 (2).mp4").exists());
  }

  #[test]
  fn publication_allocates_distinct_paths_for_parallel_downloads() {
    let target = tempfile::tempdir().unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
      let jobs = [b"first", b"other"].map(|data| {
        let (target, barrier) = (target.path(), &barrier);
        scope.spawn(move || {
          let stage = tempfile::tempdir_in(target).unwrap();
          let video = stage.path().join("clip.mp4");
          let captions = stage.path().join("clip.zh.srt");
          std::fs::write(&video, data).unwrap();
          std::fs::write(&captions, data).unwrap();
          barrier.wait();
          let path =
            super::publish_artifact_set(target, stage.path(), &video, "clip", &[captions]).unwrap();
          assert_eq!(std::fs::read(&path).unwrap(), data);
          assert_eq!(std::fs::read(path.with_extension("zh.srt")).unwrap(), data);
          path
        })
      });
      let [a, b] = jobs.map(|job| job.join().unwrap());
      assert_ne!(a, b);
    });
  }

  #[test]
  fn thumbnail_only_args_disable_other_outputs() {
    let args = super::thumbnail_only_args();
    for expected in [
      "--skip-download",
      "--write-thumbnail",
      "--convert-thumbnails",
      "jpg",
      "--no-write-subs",
      "--no-write-auto-subs",
      "--no-embed-subs",
      "--no-embed-thumbnail",
      "--no-embed-metadata",
    ] {
      assert!(args.contains(&expected), "missing {expected}");
    }
  }

  #[test]
  fn thumbnail_only_requires_existing_converted_jpeg() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = directory.path().join("manifest.jsonl");
    std::fs::write(&manifest, "null\n").unwrap();
    assert!(super::thumbnail_artifacts(&manifest).unwrap().is_empty());
    let image = directory.path().join("cover.jpg");
    std::fs::write(
      &manifest,
      serde_json::json!([{ "filepath": image }]).to_string(),
    )
    .unwrap();
    assert!(super::thumbnail_artifacts(&manifest).unwrap().is_empty());
    std::fs::write(&image, [0xff, 0xd8, 0xff, 0xd9]).unwrap();
    assert_eq!(super::thumbnail_artifacts(&manifest).unwrap(), vec![image]);
    let unconverted = directory.path().join("cover.webp");
    std::fs::write(&unconverted, b"RIFFimage").unwrap();
    std::fs::write(
      &manifest,
      serde_json::json!([{ "filepath": unconverted }]).to_string(),
    )
    .unwrap();
    assert!(super::thumbnail_artifacts(&manifest).unwrap().is_empty());
  }

  #[test]
  fn subtitle_regression_av_without_target_subtitles_completes() {
    let manifest = tempfile::NamedTempFile::new().unwrap();
    let settings = SubtitleSettings::default();
    assert!(settings.enabled);
    for data in ["", "null\n", "{}\n"] {
      std::fs::write(manifest.path(), data).unwrap();
      assert_eq!(
        finish_download_subtitles(manifest.path(), &settings, true, false, false).unwrap(),
        Vec::<std::path::PathBuf>::new()
      );
    }
  }

  #[test]
  fn subtitle_regression_av_skips_manifest_when_no_subtitle_args_were_added() {
    let manifest = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(manifest.path(), "invalid json").unwrap();
    let settings = SubtitleSettings {
      enabled: true,
      embed_subtitles: true,
      ..Default::default()
    };
    assert!(
      finish_download_subtitles(manifest.path(), &settings, false, true, false)
        .unwrap()
        .is_empty()
    );
    assert!(finish_download_subtitles(manifest.path(), &settings, true, true, false).is_err());
  }

  #[test]
  fn subtitle_regression_subtitle_only_requires_output() {
    let manifest = tempfile::NamedTempFile::new().unwrap();
    for enabled in [false, true] {
      let settings = SubtitleSettings {
        enabled,
        ..Default::default()
      };
      assert_eq!(
        format!(
          "{:?}",
          finish_download_subtitles(manifest.path(), &settings, true, false, true).unwrap_err()
        ),
        "NoSubtitles"
      );
    }
  }

  #[test]
  fn subtitle_business_payload_preserves_classification_and_internal_errors() {
    use super::super::subtitles::SubtitleError;
    let manifest = tempfile::NamedTempFile::new().unwrap();
    for data in ["", "null\n", "{}\n"] {
      std::fs::write(manifest.path(), data).unwrap();
      let error = finish_download_subtitles(
        manifest.path(),
        &SubtitleSettings::default(),
        true,
        false,
        true,
      )
      .unwrap_err();
      assert_eq!(error, SubtitleError::NoSubtitles);
      let payload = super::subtitle_fatal("group".into(), "id".into(), &error);
      assert_eq!(
        serde_json::to_value(payload).unwrap()["code"],
        "noSubtitles"
      );
    }
    for data in [
      "invalid json",
      "{\"zh\": {}}",
      "{\"zh\": {\"filepath\": \"missing.srt\"}}",
    ] {
      std::fs::write(manifest.path(), data).unwrap();
      let error = finish_download_subtitles(
        manifest.path(),
        &SubtitleSettings::default(),
        true,
        false,
        true,
      )
      .unwrap_err();
      assert!(matches!(error, SubtitleError::Internal(_)));
      let payload = super::subtitle_fatal("group".into(), "id".into(), &error);
      assert!(payload.code.is_none());
      assert!(payload.details.is_some());
    }
  }

  #[test]
  fn subtitle_regression_av_invalid_existing_output_still_fails() {
    let directory = tempfile::tempdir().unwrap();
    let subtitle = directory.path().join("video.zh-Hant.srt");
    let manifest = directory.path().join("manifest.jsonl");
    std::fs::write(
      &manifest,
      serde_json::json!({"zh-Hant": {"filepath": subtitle}}).to_string(),
    )
    .unwrap();
    assert!(matches!(
      finish_download_subtitles(&manifest, &SubtitleSettings::default(), true, false, false),
      Err(super::super::subtitles::SubtitleError::Internal(_))
    ));
    std::fs::write(&subtitle, "invalid SRT").unwrap();
    assert_eq!(
      finish_download_subtitles(&manifest, &SubtitleSettings::default(), true, false, false)
        .unwrap_err()
        .to_string(),
      "Subtitle output is empty or invalid SRT"
    );
  }
}
