use crate::logging::LogStoreState;
use crate::models::download::DownloadSection;
use crate::models::{
  MediaDiagnosticPayload, MediaFatalPayload, MediaProgressComplete, ProgressEvent,
};
use crate::parsers::ytdlp_error::{DiagnosticMatcher, YtdlpErrorParser};
use crate::parsers::ytdlp_progress::{progress_category_for_track_type, YtdlpProgressParser};
use crate::runners::ytdlp_runner::{YtdlpCommandEvent, YtdlpRunner};
use crate::scheduling::download_pipeline::DownloadEntry;
use crate::scheduling::group_state::subscribe_group;
use std::fmt;
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

pub async fn run_ytdlp_download(
  app: AppHandle,
  entry: DownloadEntry,
) -> Result<(), YtdlpDownloadError> {
  let manifest = tempfile::NamedTempFile::new()
    .map_err(|e| YtdlpDownloadError::InvalidArguments(e.to_string()))?;
  let media_manifest = tempfile::NamedTempFile::new()
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
  let runner = if entry.thumbnail_only {
    runner.with_args(thumbnail_only_args())
  } else {
    runner.with_subtitle_args(entry.overrides.as_ref(), entry.subtitle_inventory.as_ref())
  };
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
  let runner = runner
    .with_args(output_args)
    .with_location_args(
      &entry.format.track_type,
      &entry.template_context,
      entry.overrides.as_ref(),
    )
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
      if entry.thumbnail_only {
        "after_video:%(thumbnails)j".into()
      } else {
        "after_move:%(filepath)j".into()
      },
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

  let (mut rx, child) = match runner.spawn() {
    Ok(result) => result,
    Err(err) => {
      let message = format!("Failed to spawn yt-dlp: {err}");
      let _ = app.emit(
        "media_fatal",
        MediaFatalPayload::with_exit(entry.group_id.clone(), entry.id.clone(), 1, message),
      );
      return Err(YtdlpDownloadError::SpawnFailed(err));
    }
  };
  let mut cancel_rx = subscribe_group(&entry.group_id);

  loop {
    tokio::select! {
      event = rx.recv() => {
        let Some(event) = event else {
          break;
        };

        if is_cancelled_now(&cancel_rx) {
          tracing::info!("Cancelled processing for group_id {}", entry.group_id);
          let _ = child.kill_tree();
          return Ok(());
        }

        let log_state = app.state::<LogStoreState>();

        match event {
          YtdlpCommandEvent::Stdout(line) => {
            let line_str = String::from_utf8_lossy(&line);
            store_log_line(&line_str, &entry, log_state, &app);
            parse_progress_line(&line_str, &mut progress_parser, &app);
            parse_error_line(&line_str, &error_parser, &app);
          }
          YtdlpCommandEvent::Stderr(line) => {
            let line_str = String::from_utf8_lossy(&line);
            store_log_line(&line_str, &entry, log_state, &app);
            parse_progress_line(&line_str, &mut progress_parser, &app);
            parse_error_line(&line_str, &error_parser, &app);
          }
          YtdlpCommandEvent::Terminated(term) => {
            if term.code == Some(0) {
              if entry.thumbnail_only {
                let image = match finish_thumbnail(media_manifest.path()) {
                  Ok(image) => image,
                  Err(err) => {
                    emit_internal_fatal(&app, &entry, format!("Thumbnail processing failed: {err}"), Some(err.clone()));
                    return Err(YtdlpDownloadError::RunnerError(err));
                  }
                };
                if is_cancelled_now(&cancel_rx) { return Ok(()); }
                let _ = app.emit("media_destination", crate::models::MediaDestination {
                  id: entry.id.clone(),
                  group_id: entry.group_id.clone(),
                  destination: crate::models::progress::MediaDestinationPath {
                    confidence: 100,
                    path: image.to_string_lossy().into_owned(),
                  },
                  is_merged: false,
                });
              } else {
              let explicit_tracks = entry.overrides.as_ref().and_then(|value| value.subtitles.as_ref()).is_some_and(|value| value.manual_languages.is_some() || value.auto_languages.is_some());
              let result = finish_download_subtitles(manifest.path(), &subtitle_settings, explicit_tracks, entry.subtitles_only);
              let subtitles = match result {
                Ok(files) => files,
                Err(err) => {
                  emit_internal_fatal(&app, &entry, format!("Subtitle processing failed: {err}"), Some(err.clone()));
                  return Err(YtdlpDownloadError::RunnerError(err));
                }
              };
              if !entry.subtitles_only && subtitle_settings.embed_subtitles && !subtitles.is_empty() {
                if let Err(err) = embed_subtitles(&app, media_manifest.path(), &subtitles, &mut cancel_rx).await {
                  if is_cancelled_now(&cancel_rx) { return Ok(()); }
                  emit_internal_fatal(&app, &entry, format!("Subtitle embedding failed: {err}"), Some(err.clone()));
                  return Err(YtdlpDownloadError::RunnerError(err));
                }
              }
              }
              if is_cancelled_now(&cancel_rx) { return Ok(()); }
              let _ = app.emit(
                "media_complete",
                MediaProgressComplete {
                  id: entry.id.clone(),
                  group_id: entry.group_id.clone(),
                },
              );
              return Ok(());
            }

            let exit = term.code.unwrap_or(1);
            let _ = app.emit(
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
            let msg = format!("Download failed for group {}: {}", entry.group_id, err);
            emit_internal_fatal(&app, &entry, msg.clone(), Some(err.clone()));

            return Err(YtdlpDownloadError::RunnerError(err));
          }
        }
      }
      _ = cancel_rx.changed() => {
        if is_cancelled_now(&cancel_rx) {
          tracing::info!("Cancelled processing for group_id {}", entry.group_id);
          let _ = child.kill_tree();
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

fn finish_thumbnail(manifest: &std::path::Path) -> Result<std::path::PathBuf, String> {
  use std::io::Read;
  let contents = std::fs::read_to_string(manifest).map_err(|err| err.to_string())?;
  for line in contents.lines().filter(|line| !line.trim().is_empty()) {
    let thumbnails: serde_json::Value =
      serde_json::from_str(line).map_err(|err| err.to_string())?;
    if let Some(thumbnails) = thumbnails.as_array() {
      for thumbnail in thumbnails.iter().rev() {
        let Some(path) = thumbnail.get("filepath").and_then(|value| value.as_str()) else {
          continue;
        };
        let path = std::path::PathBuf::from(path);
        if !path
          .extension()
          .is_some_and(|ext| ext.eq_ignore_ascii_case("jpg"))
        {
          continue;
        }
        let mut header = [0; 3];
        if std::fs::File::open(&path)
          .and_then(|mut file| file.read_exact(&mut header))
          .is_ok()
          && header == [0xff, 0xd8, 0xff]
        {
          return std::fs::canonicalize(path).map_err(|err| err.to_string());
        }
      }
    }
  }
  Err("No JPEG thumbnail was downloaded or thumbnail conversion failed".into())
}

fn finish_download_subtitles(
  manifest: &std::path::Path,
  settings: &crate::state::config_models::SubtitleSettings,
  explicit_tracks: bool,
  subtitles_only: bool,
) -> Result<Vec<std::path::PathBuf>, String> {
  crate::runners::subtitles::finish_subtitles(
    manifest,
    settings.chinese_priority && !explicit_tracks,
    subtitles_only,
  )
}

async fn embed_subtitles(
  app: &AppHandle,
  manifest: &std::path::Path,
  subtitles: &[std::path::PathBuf],
  cancel: &mut watch::Receiver<bool>,
) -> Result<(), String> {
  let resolver =
    crate::binaries::tool_resolver::ToolResolver::new(&app.state::<crate::paths::PathsManager>());
  let binary = tokio::task::spawn_blocking(move || resolver.ffmpeg_pair().0)
    .await
    .map_err(|e| e.to_string())?
    .ok_or("No usable ffmpeg found")?;
  super::subtitle_embed::embed_with_binary(&binary, manifest, subtitles, cancel).await
}

fn emit_internal_fatal(
  app: &AppHandle,
  entry: &DownloadEntry,
  message: String,
  details: Option<String>,
) {
  let _ = app.emit(
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

fn parse_progress_line(line: &str, progress_parser: &mut YtdlpProgressParser, app: &AppHandle) {
  let progress_events = progress_parser.parse_line(line);

  for progress_event in progress_events {
    match progress_event {
      ProgressEvent::Destination(destination) => {
        app.emit("media_destination", destination).ok();
      }
      ProgressEvent::Progress(progress) => {
        app.emit("media_progress", progress).ok();
      }
      ProgressEvent::StageChange(progress) => {
        app.emit("media_progress_stage", progress).ok();
      }
    }
  }
}

fn parse_error_line(line: &str, error_parser: &YtdlpErrorParser, app: &AppHandle) {
  if let Some(event) = error_parser.parse_line(line) {
    app
      .emit(
        "media_diagnostic",
        MediaDiagnosticPayload::from_diagnostic_event(event),
      )
      .ok();
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
    assert!(super::finish_thumbnail(&manifest).is_err());
    let image = directory.path().join("cover.jpg");
    std::fs::write(
      &manifest,
      serde_json::json!([{ "filepath": image }]).to_string(),
    )
    .unwrap();
    assert!(super::finish_thumbnail(&manifest).is_err());
    std::fs::write(&image, [0xff, 0xd8, 0xff, 0xd9]).unwrap();
    assert_eq!(
      super::finish_thumbnail(&manifest).unwrap(),
      std::fs::canonicalize(image).unwrap()
    );
    let unconverted = directory.path().join("cover.webp");
    std::fs::write(&unconverted, b"RIFFimage").unwrap();
    std::fs::write(
      &manifest,
      serde_json::json!([{ "filepath": unconverted }]).to_string(),
    )
    .unwrap();
    assert!(super::finish_thumbnail(&manifest).is_err());
  }

  #[test]
  fn subtitle_regression_av_without_target_subtitles_completes() {
    let manifest = tempfile::NamedTempFile::new().unwrap();
    let settings = SubtitleSettings::default();
    assert!(settings.enabled);
    for data in ["", "null\n", "{}\n"] {
      std::fs::write(manifest.path(), data).unwrap();
      assert_eq!(
        finish_download_subtitles(manifest.path(), &settings, false, false).unwrap(),
        Vec::<std::path::PathBuf>::new()
      );
    }
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
        finish_download_subtitles(manifest.path(), &settings, false, true).unwrap_err(),
        "No subtitle tracks were downloaded"
      );
    }
  }

  #[test]
  fn subtitle_regression_av_invalid_existing_output_still_fails() {
    let directory = tempfile::tempdir().unwrap();
    let subtitle = directory.path().join("video.zh-Hant.srt");
    let manifest = directory.path().join("manifest.jsonl");
    std::fs::write(&subtitle, "invalid SRT").unwrap();
    std::fs::write(
      &manifest,
      serde_json::json!({"zh-Hant": {"filepath": subtitle}}).to_string(),
    )
    .unwrap();
    assert_eq!(
      finish_download_subtitles(&manifest, &SubtitleSettings::default(), false, false).unwrap_err(),
      "Subtitle output is empty or invalid SRT"
    );
  }
}
