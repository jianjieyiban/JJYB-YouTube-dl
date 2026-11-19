use crate::binaries::tool_resolver::ToolResolver;
use crate::models::download::{DownloadOverrides, FormatOptions, PlaylistMode};
use crate::models::SubtitleInventory;
use crate::models::TrackType;
use crate::paths::PathsManager;
use crate::runners::override_resolver::resolve_with_patch;
use crate::runners::template_context::TemplateContext;
use crate::runners::ytdlp_args::{
  build_format_args, build_input_filter_args, build_output_args, build_output_template,
};
use crate::runners::ytdlp_process::{
  configure_command, kill_platform_process, platform_process_from_child,
};
use crate::state::config_models::{AuthSettings, Config, SponsorBlockSettings, SubtitleSettings};
use crate::state::preferences_models::Preferences;
use crate::stronghold::stronghold_state::{AuthSecrets, StrongholdState};
use crate::{SharedConfig, SharedPreferences};
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

#[derive(Debug, Clone)]
pub struct TerminatedPayload {
  pub code: Option<i32>,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum YtdlpCommandEvent {
  Stderr(Vec<u8>),
  Stdout(Vec<u8>),
  Error(String),
  Terminated(TerminatedPayload),
}

#[derive(Debug)]
pub struct YtdlpOutput {
  pub status: ExitStatus,
  pub stdout: Vec<u8>,
  pub stderr: Vec<u8>,
}

#[derive(Debug)]
pub struct YtdlpChild {
  shutdown: mpsc::Sender<()>,
  reaped: tokio::sync::oneshot::Receiver<()>,
}

pub struct YtdlpRunner<'a> {
  app: &'a AppHandle,
  cfg: Arc<Config>,
  prefs: Arc<Preferences>,
  args: Vec<String>,
  resolver: ToolResolver,
}

impl<'a> YtdlpRunner<'a> {
  pub fn new(app: &'a AppHandle) -> Self {
    let paths_manager = app.state::<PathsManager>();
    let resolver = ToolResolver::new(&paths_manager);
    let args = vec!["--encoding".into(), "utf-8".into()];
    let cfg_handle = app.state::<SharedConfig>();
    let cfg = cfg_handle.load();
    let prefs_handle = app.state::<SharedPreferences>();
    let prefs = prefs_handle.load();

    Self {
      app,
      cfg,
      prefs,
      args,
      resolver,
    }
  }

  pub fn with_args<I, S>(mut self, args: I) -> Self
  where
    I: IntoIterator<Item = S>,
    S: Into<String>,
  {
    self.args.extend(args.into_iter().map(Into::into));
    self
  }

  pub fn with_network_args(mut self, overrides: Option<&DownloadOverrides>) -> Self {
    let network = resolve_with_patch(
      &self.cfg.network,
      overrides.and_then(|value| value.network.as_ref()),
    );
    let proxy_enabled = network.enable_proxy.is_some_and(|enabled| enabled);
    if proxy_enabled {
      if let Some(proxy) = network.proxy.as_ref() {
        self.args.push("--proxy".to_string());
        self.args.push(proxy.clone());
      }
    }

    match network.impersonate.as_str() {
      "none" => {}
      "any" => {
        self.args.push("--impersonate".to_string());
        self.args.push(String::new());
      }
      other => {
        self.args.push("--impersonate".to_string());
        self.args.push(other.to_string());
      }
    }

    if let Some(extractor_args) = normalize_extractor_args(&network.extractor_args) {
      self.args.push("--extractor-args".into());
      self.args.push(extractor_args);
    }

    self
  }

  pub fn with_auth_args(mut self, overrides: Option<&DownloadOverrides>) -> Self {
    let auth_overrides = overrides.and_then(|value| value.auth.as_ref());
    let auth_settings: AuthSettings = resolve_with_patch(&self.cfg.auth, auth_overrides);
    if auth_settings.cookie_browser != "none" {
      self.args.extend_from_slice(&[
        "--cookies-from-browser".into(),
        auth_settings.cookie_browser,
      ]);
    }
    if let Some(cookie_file) = auth_settings.cookie_file {
      self
        .args
        .extend_from_slice(&["--cookies".into(), cookie_file]);
    }

    let mut effective_auth_secrets = AuthSecrets::default();
    if let Some(sh_state) = self.app.try_state::<StrongholdState>() {
      if let Ok(secrets) = sh_state.load_auth_secrets() {
        effective_auth_secrets = secrets;
      }
    }
    let effective_auth_secrets = resolve_with_patch(&effective_auth_secrets, auth_overrides);
    self.apply_auth_secrets(effective_auth_secrets);

    self
  }

  pub fn with_subtitle_args(
    mut self,
    overrides: Option<&DownloadOverrides>,
    subtitle_inventory: Option<&SubtitleInventory>,
  ) -> (Self, bool) {
    let subtitle_overrides = overrides.and_then(|value| value.subtitles.as_ref());
    let subtitle_settings = resolve_with_patch(&self.cfg.subtitles, subtitle_overrides);
    if let Some(subtitle_args) =
      build_subtitle_args(&subtitle_settings, subtitle_overrides, subtitle_inventory)
    {
      self.args.extend(subtitle_args);
      (self, true)
    } else {
      (self, false)
    }
  }

  pub fn with_sponsorblock_args(mut self, overrides: Option<&DownloadOverrides>) -> Self {
    let sponsor_block = resolve_with_patch(
      &self.cfg.sponsor_block,
      overrides.and_then(|value| value.sponsor_block.as_ref()),
    );
    let output_settings = resolve_with_patch(
      &self.cfg.output,
      overrides.and_then(|value| value.output.as_ref()),
    );
    self.args.extend(build_sponsorblock_args(
      &sponsor_block,
      output_settings.precise_cuts,
    ));

    self
  }

  pub fn with_progress_args(mut self) -> Self {
    self.args.extend_from_slice(&[
            "--newline".into(),
            "--progress".into(),
            "--no-color".into(),
            "--progress-template".into(),
            "RAW|%(progress.percent|)s|%(progress._percent_str|)s|%(progress.speed|)s|%(progress.eta|)s|%(progress.downloaded_bytes|)s|%(progress.total_bytes|)s|%(progress.total_bytes_estimate|)s|%(progress.fragment_index|)s|%(progress.fragment_count|)s".into(),
            "--progress-delta".into(),
            "0.5".into(),
        ]);
    self
  }

  pub fn with_format_args(
    mut self,
    format_options: &FormatOptions,
    overrides: Option<&DownloadOverrides>,
  ) -> Self {
    let output_settings = resolve_with_patch(
      &self.cfg.output,
      overrides.and_then(|value| value.output.as_ref()),
    );
    self
      .args
      .extend(build_format_args(format_options, &output_settings));
    self
  }

  pub fn with_input_args(mut self, overrides: Option<&DownloadOverrides>) -> Self {
    let playlist_mode = overrides
      .and_then(|value| value.input_filters.as_ref())
      .and_then(|value| value.playlist_mode);

    match playlist_mode {
      Some(PlaylistMode::SingleVideo) => self.args.push("--no-playlist".into()),
      Some(PlaylistMode::Playlist) => self.args.push("--yes-playlist".into()),
      None => {
        let input = resolve_with_patch(
          &self.cfg.input,
          overrides.and_then(|value| value.input.as_ref()),
        );
        if input.prefer_video_in_mixed_links {
          self.args.push("--no-playlist".into());
        } else {
          self.args.push("--yes-playlist".into());
        }
      }
    }
    self
  }

  pub fn with_input_filter_args(mut self, overrides: Option<&DownloadOverrides>) -> Self {
    self.args.extend(build_input_filter_args(
      overrides.and_then(|value| value.input_filters.as_ref()),
    ));
    self
  }

  pub fn output_args(
    &self,
    format_options: &FormatOptions,
    overrides: Option<&DownloadOverrides>,
  ) -> Result<Vec<String>, String> {
    let output_overrides = overrides.and_then(|value| value.output.as_ref());
    let output_settings = resolve_with_patch(&self.cfg.output, output_overrides);
    let partial_download = output_overrides.and_then(|value| value.partial_download.as_ref());
    build_output_args(format_options, &output_settings, partial_download)
  }

  pub fn output_template(
    &self,
    track_type: &TrackType,
    template_context: &TemplateContext,
    overrides: Option<&DownloadOverrides>,
  ) -> PathBuf {
    let output_settings = resolve_with_patch(
      &self.cfg.output,
      overrides.and_then(|value| value.output.as_ref()),
    );
    let fallback_dir = self
      .app
      .path()
      .download_dir()
      .unwrap_or_else(|_| std::env::current_dir().expect("couldn’t get current dir"));
    build_output_template(
      track_type,
      template_context,
      &output_settings,
      &self.prefs.paths,
      fallback_dir,
    )
  }

  pub fn with_url(mut self, url: &str) -> Self {
    self.args.push(url.into());
    self
  }

  pub async fn output(self) -> Result<YtdlpOutput, String> {
    log_run_summary(&self.args);
    tracing::debug!("Running command: yt-dlp {}", self.args.join(" "));
    let mut command = self.build_command()?;

    configure_command(&mut command).map_err(|e| format!("yt-dlp spawn setup failed: {e}"))?;

    tauri::async_runtime::spawn_blocking(move || {
      let output = command
        .output()
        .map_err(|e| format!("yt-dlp failed to run: {e}"))?;
      Ok(YtdlpOutput {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
      })
    })
    .await
    .map_err(|e| format!("yt-dlp task failed: {e}"))?
  }

  pub fn spawn(self) -> Result<(UnboundedReceiver<YtdlpCommandEvent>, YtdlpChild), String> {
    log_run_summary(&self.args);
    tracing::debug!("Running command: yt-dlp {}", self.args.join(" "));
    spawn_download_command(self.build_command()?).map(|(rx, child, _)| (rx, child))
  }

  fn build_command(&self) -> Result<Command, String> {
    let binary = self
      .resolver
      .resolve("yt-dlp")
      .ok_or("No usable yt-dlp found")?;
    let mut command = Command::new(binary);
    command.args(&self.args);
    let (ffmpeg, ffprobe) = self.resolver.ffmpeg_pair();
    if let Some(ref encoder) = ffmpeg {
      command.arg("--ffmpeg-location").arg(encoder);
    }
    if let Some(deno) = self.resolver.resolve("deno") {
      command
        .arg("--js-runtimes")
        .arg(format!("deno:{}", deno.display()));
    }
    let mut dirs: Vec<PathBuf> = ffmpeg
      .iter()
      .chain(ffprobe.iter())
      .filter_map(|path| path.parent().map(|parent| parent.to_path_buf()))
      .collect();
    dirs.extend(self.resolver.tool_dirs().iter().cloned());
    if let Some(path) = std::env::var_os("PATH") {
      dirs.extend(std::env::split_paths(&path));
    }
    command.env(
      "PATH",
      std::env::join_paths(dirs).map_err(|e| e.to_string())?,
    );
    Ok(command)
  }

  fn apply_auth_secrets(&mut self, s: AuthSecrets) {
    if let Some(u) = s.username {
      self.args.push("--username".into());
      self.args.push(u);
    }
    if let Some(p) = s.password {
      self.args.push("--password".into());
      self.args.push(p);
    }
    if let Some(vp) = s.video_password {
      self.args.push("--video-password".into());
      self.args.push(vp);
    }
    if let Some(token) = s.bearer_token {
      self.args.push("--add-header".into());
      self.args.push(format!("Authorization:Bearer {token}"));
    }
    for h in s.headers {
      self.args.push("--add-header".into());
      self.args.push(h);
    }
  }
}

fn spawn_download_command(
  mut command: Command,
) -> Result<(UnboundedReceiver<YtdlpCommandEvent>, YtdlpChild, u32), String> {
  command
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

  configure_command(&mut command).map_err(|e| format!("yt-dlp spawn setup failed: {e}"))?;

  let mut raw_child = command
    .spawn()
    .map_err(|e| format!("yt-dlp failed to spawn: {e}"))?;
  let pid = raw_child.id();
  let stdout = raw_child.stdout.take();
  let stderr = raw_child.stderr.take();

  let platform = match platform_process_from_child(&raw_child) {
    Ok(platform) => platform,
    Err(err) => {
      let _ = raw_child.kill();
      let _ = raw_child.wait();
      return Err(err);
    }
  };

  let (tx, rx) = unbounded_channel();

  let stdout = stdout.map(|reader| spawn_reader(reader, tx.clone(), true));
  let stderr = stderr.map(|reader| spawn_reader(reader, tx.clone(), false));

  let (shutdown, shutdown_rx) = mpsc::channel();
  let (reaped_tx, reaped) = tokio::sync::oneshot::channel();
  thread::spawn(move || {
    let status = loop {
      match shutdown_rx.recv_timeout(Duration::from_millis(20)) {
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
          kill_platform_process(&platform);
          let _ = raw_child.kill();
          // A descendant that escaped the group could still hold a pipe.
          // Reap the leader without waiting for the reader threads.
          break raw_child.wait();
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
          // Drain diagnostics before reporting termination. Until then, keep
          // the leader unreaped so cancellation can safely signal the group.
          if stdout
            .iter()
            .chain(stderr.iter())
            .all(|reader| reader.is_finished())
          {
            match raw_child.try_wait() {
              Ok(None) => continue,
              Ok(Some(status)) => break Ok(status),
              Err(err) => break Err(err),
            }
          }
        }
      }
    };
    #[cfg(windows)]
    drop(platform);
    if status.is_ok() {
      let _ = reaped_tx.send(());
    }
    match status {
      Ok(status) => {
        let payload = TerminatedPayload {
          code: status.code(),
        };
        let _ = tx.send(YtdlpCommandEvent::Terminated(payload));
      }
      Err(err) => {
        let _ = tx.send(YtdlpCommandEvent::Error(err.to_string()));
      }
    }
  });

  let child = YtdlpChild { shutdown, reaped };

  Ok((rx, child, pid))
}

/// Presence-only summary of a yt-dlp invocation, safe to log or send to
/// Sentry at any log level. Unlike a redaction blacklist, an omission here
/// can only under-report metadata — it can never leak a secret value,
/// because no argument *values* are ever inspected or stored, only whether
/// certain flags are present.
struct RunLogSummary {
  arg_count: usize,
  has_proxy: bool,
  has_cookies: bool,
  has_browser_cookies: bool,
  has_auth: bool,
}

fn summarize_args_for_log(args: &[String]) -> RunLogSummary {
  RunLogSummary {
    arg_count: args.len(),
    has_proxy: args
      .iter()
      .any(|arg| arg == "--proxy" || arg.starts_with("--proxy=")),
    has_cookies: args
      .iter()
      .any(|arg| arg == "--cookies" || arg.starts_with("--cookies=")),
    has_browser_cookies: args
      .iter()
      .any(|arg| arg == "--cookies-from-browser" || arg.starts_with("--cookies-from-browser=")),
    has_auth: args.iter().any(|arg| {
      matches!(
        arg.as_str(),
        "--username" | "--password" | "--video-password" | "--add-header"
      ) || arg.starts_with("--username=")
        || arg.starts_with("--password=")
        || arg.starts_with("--video-password=")
        || arg.starts_with("--add-header=")
    }),
  }
}

fn log_run_summary(args: &[String]) {
  let summary = summarize_args_for_log(args);
  tracing::info!(
    arg_count = summary.arg_count,
    has_proxy = summary.has_proxy,
    has_cookies = summary.has_cookies,
    has_browser_cookies = summary.has_browser_cookies,
    has_auth = summary.has_auth,
    "Running yt-dlp command"
  );
}

impl YtdlpChild {
  pub async fn shutdown(&mut self) -> Result<(), tokio::sync::oneshot::error::RecvError> {
    let _ = self.shutdown.send(());
    // A timeout alone must not release staging for cleanup or reuse.
    match tokio::time::timeout(Duration::from_secs(3), &mut self.reaped).await {
      Ok(result) => result,
      Err(_) => {
        tracing::warn!("Still waiting for yt-dlp to exit before download cleanup");
        (&mut self.reaped).await
      }
    }
  }
}

fn spawn_reader<R: Read + Send + 'static>(
  reader: R,
  tx: UnboundedSender<YtdlpCommandEvent>,
  is_stdout: bool,
) -> thread::JoinHandle<()> {
  thread::spawn(move || {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    let mut byte = [0_u8; 1];

    loop {
      match reader.read(&mut byte) {
        Ok(0) => {
          if !buf.is_empty() {
            let out = std::mem::take(&mut buf);
            let event = if is_stdout {
              YtdlpCommandEvent::Stdout(out)
            } else {
              YtdlpCommandEvent::Stderr(out)
            };
            let _ = tx.send(event);
          }
          break;
        }
        Ok(_) if matches!(byte[0], b'\n' | b'\r') => {
          if buf.is_empty() {
            continue;
          }

          let out = std::mem::take(&mut buf);
          let event = if is_stdout {
            YtdlpCommandEvent::Stdout(out)
          } else {
            YtdlpCommandEvent::Stderr(out)
          };
          let _ = tx.send(event);
        }
        Ok(_) => buf.push(byte[0]),
        Err(err) => {
          let _ = tx.send(YtdlpCommandEvent::Error(err.to_string()));
          break;
        }
      }
    }
  })
}

fn build_subtitle_args(
  settings: &SubtitleSettings,
  overrides: Option<&crate::models::download::SubtitleOverrides>,
  subtitle_inventory: Option<&SubtitleInventory>,
) -> Option<Vec<String>> {
  if !settings.enabled {
    return None;
  }

  let resolution = resolve_subtitle_request(settings, overrides, subtitle_inventory)?;

  let mut args = vec!["--write-subs".into()];

  args.extend([
    "--no-embed-subs".into(),
    "--convert-subs".into(),
    "srt".into(),
  ]);

  if resolution.include_auto_generated {
    args.push("--write-auto-subs".into());
  }

  let formats = sanitize_subtitle_formats(&settings.format_preference);
  args.push("--sub-format".into());
  args.push(formats.join("/"));

  args.push("--sub-langs".into());
  args.push(
    resolution
      .languages
      .iter()
      .map(|id| {
        if id == "all" {
          "all".to_string()
        } else {
          format!("^{}$", regex::escape(id))
        }
      })
      .collect::<Vec<_>>()
      .join(","),
  );

  Some(args)
}

fn normalize_extractor_args(value: &str) -> Option<String> {
  let normalized = value
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .collect::<Vec<_>>()
    .join(" ");
  if normalized.is_empty() {
    None
  } else {
    Some(normalized)
  }
}

fn build_sponsorblock_args(settings: &SponsorBlockSettings, precise_cuts: bool) -> Vec<String> {
  let mut args = Vec::new();

  if let Some(api_url) = &settings.api_url {
    args.extend_from_slice(&["--sponsorblock-api".into(), api_url.clone()]);
  }

  if !settings.remove_parts.is_empty() {
    args.extend_from_slice(&[
      "--sponsorblock-remove".into(),
      settings.remove_parts.join(","),
    ]);
    if precise_cuts {
      args.push("--force-keyframes-at-cuts".into());
    }
  }

  if !settings.mark_parts.is_empty() {
    args.extend_from_slice(&["--sponsorblock-mark".into(), settings.mark_parts.join(",")]);
  }

  args
}

fn sanitize_subtitle_formats(formats: &[String]) -> Vec<String> {
  const DEFAULT_FORMATS: [&str; 5] = ["srt", "vtt", "ass", "ttml", "json3"];

  let normalized_inputs = sanitize_vec(formats)
    .into_iter()
    .map(|value| {
      if value == "json" {
        "json3".to_string()
      } else {
        value
      }
    })
    .fold(Vec::new(), |mut acc, value| {
      if !acc.contains(&value) {
        acc.push(value);
      }
      acc
    });

  let primary = normalized_inputs
    .iter()
    .find(|value| DEFAULT_FORMATS.contains(&value.as_str()))
    .cloned()
    .unwrap_or_else(|| {
      normalized_inputs
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_FORMATS[0].to_string())
    });

  let mut ordered = Vec::new();
  let mut seen = HashSet::new();

  if seen.insert(primary.clone()) {
    ordered.push(primary);
  }

  for fallback in &DEFAULT_FORMATS {
    let normalized = (*fallback).to_string();
    if seen.insert(normalized.clone()) {
      ordered.push(normalized);
    }
  }

  for extra in normalized_inputs {
    if seen.insert(extra.clone()) {
      ordered.push(extra);
    }
  }

  ordered
}

fn sanitize_subtitle_languages(languages: &[String]) -> Vec<String> {
  let mut sanitized = sanitize_vec(languages);

  if sanitized.is_empty() {
    sanitized.push("en".into());
  }

  sanitized
}

fn sanitize_vec(items: &[String]) -> Vec<String> {
  let mut seen = HashSet::new();
  let mut sanitized = Vec::new();

  for item in items {
    let trimmed = item.trim();
    if trimmed.is_empty() {
      continue;
    }

    let key = trimmed.to_ascii_lowercase();
    if seen.insert(key.clone()) {
      sanitized.push(key);
    }
  }

  sanitized
}

struct ResolvedSubtitleRequest {
  languages: Vec<String>,
  include_auto_generated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoLanguagePreference {
  request: String,
  base: String,
}

fn resolve_subtitle_request(
  settings: &SubtitleSettings,
  overrides: Option<&crate::models::download::SubtitleOverrides>,
  subtitle_inventory: Option<&SubtitleInventory>,
) -> Option<ResolvedSubtitleRequest> {
  let inventory = subtitle_inventory?;
  if settings.chinese_priority
    && !overrides
      .is_some_and(|value| value.manual_languages.is_some() || value.auto_languages.is_some())
  {
    return resolve_chinese_priority(settings, inventory);
  }
  let manual = inventory.manual_languages.clone();
  let automatic = inventory.auto_languages.clone();
  let preferred_automatic = preferred_auto_languages(&automatic);

  if let Some(overrides) = overrides {
    let has_source_specific_languages =
      overrides.manual_languages.is_some() || overrides.auto_languages.is_some();
    if has_source_specific_languages {
      let manual_languages = resolve_manual_override_languages(
        overrides.manual_languages.as_deref().unwrap_or(&[]),
        &manual,
      );
      let auto_languages = resolve_auto_override_languages(
        overrides.auto_languages.as_deref().unwrap_or(&[]),
        &automatic,
      );
      let languages = available_subtitle_languages(&manual_languages, &auto_languages);
      if languages.is_empty() {
        return None;
      }

      return Some(ResolvedSubtitleRequest {
        languages,
        include_auto_generated: !auto_languages.is_empty(),
      });
    }
  }

  let requested = sanitize_subtitle_languages(&settings.languages);
  let wants_all = requested.iter().any(|language| language == "all");
  let manual_languages = if wants_all {
    manual.clone()
  } else {
    resolve_manual_override_languages(&requested, &manual)
  };
  let auto_languages = if settings.include_auto_generated {
    if wants_all {
      preferred_automatic
        .iter()
        .map(|language| language.request.clone())
        .collect::<Vec<_>>()
    } else {
      resolve_auto_subtitle_languages(&requested, &preferred_automatic)
    }
  } else {
    Vec::new()
  };

  let safe_for_auto = !auto_languages.is_empty() && {
    let manual_bases = subtitle_language_bases(&manual_languages);
    let auto_bases = subtitle_language_bases(&auto_languages);
    manual_bases.is_empty() || manual_bases == auto_bases
  };
  let languages = if safe_for_auto {
    available_subtitle_languages(&manual_languages, &auto_languages)
  } else {
    manual_languages
  };

  if languages.is_empty() {
    None
  } else {
    Some(ResolvedSubtitleRequest {
      languages,
      include_auto_generated: safe_for_auto,
    })
  }
}

fn resolve_chinese_priority(
  settings: &SubtitleSettings,
  inventory: &SubtitleInventory,
) -> Option<ResolvedSubtitleRequest> {
  let mut languages = Vec::new();
  let mut include_auto_generated = false;
  let manual = &inventory.manual_languages;
  let automatic = &inventory.auto_languages;
  let has_orig = automatic.iter().any(|id| id.ends_with("-orig"));
  let requested_languages = std::iter::once("zh-Hans".to_string()).chain(
    settings
      .languages
      .iter()
      .filter(|id| !subtitle_language_base(id).starts_with("zh"))
      .cloned(),
  );
  for requested in requested_languages {
    let base = subtitle_language_base(requested.trim());
    let chinese = base == "zh" || base.starts_with("zh-");
    let matches = |id: &str, traditional: bool| {
      let id = subtitle_language_base(id);
      if chinese {
        if traditional {
          matches!(id.as_str(), "zh-hant" | "zh-tw" | "zh-hk" | "zh-mo")
        } else {
          matches!(id.as_str(), "zh" | "zh-hans" | "zh-cn" | "zh-sg")
        }
      } else {
        base == "all" || id == base
      }
    };
    let mut selected = None;
    for traditional in [false, true] {
      if traditional && !chinese {
        break;
      }
      if let Some(id) = manual.iter().find(|id| matches(id, traditional)) {
        selected = Some((id.clone(), false));
        break;
      }
      if chinese || settings.include_auto_generated {
        if let Some(id) = automatic
          .iter()
          .find(|id| matches(id, traditional) && (!has_orig || id.ends_with("-orig")))
        {
          selected = Some((id.clone(), true));
          break;
        }
      }
    }
    if selected.is_none() && (chinese || settings.include_auto_generated) {
      selected = automatic
        .iter()
        .find(|id| matches(id, false))
        .map(|id| (id.clone(), true));
      if selected.is_none() && chinese {
        for targets in [
          &["zh-hans", "zh-cn", "zh-sg"][..],
          &["zh-hant", "zh-tw", "zh-hk", "zh-mo"][..],
        ] {
          if let Some(id) = automatic.iter().find(|id| {
            let normalized = id.to_ascii_lowercase();
            targets
              .iter()
              .any(|target| normalized.starts_with(&format!("{target}-")))
          }) {
            selected = Some((id.clone(), true));
            break;
          }
        }
      }
    }
    if let Some((id, automatic)) = selected {
      include_auto_generated |= automatic;
      if !languages.contains(&id) {
        languages.push(id);
      }
    }
  }
  (!languages.is_empty()).then_some(ResolvedSubtitleRequest {
    languages,
    include_auto_generated,
  })
}

fn resolve_manual_override_languages(requested: &[String], available: &[String]) -> Vec<String> {
  let requested = sanitize_vec(requested);
  if requested.iter().any(|language| language == "all") {
    return available.to_vec();
  }

  requested
    .iter()
    .filter_map(|language| {
      available
        .iter()
        .find(|id| id.eq_ignore_ascii_case(language))
        .cloned()
    })
    .collect()
}

fn resolve_auto_override_languages(requested: &[String], automatic: &[String]) -> Vec<String> {
  resolve_manual_override_languages(requested, automatic)
}

fn available_subtitle_languages(manual: &[String], automatic: &[String]) -> Vec<String> {
  let mut seen = HashSet::new();
  let mut available = Vec::new();

  for language in manual.iter().chain(automatic.iter()) {
    if seen.insert(language.clone()) {
      available.push(language.clone());
    }
  }

  available
}

fn resolve_auto_subtitle_languages(
  requested: &[String],
  automatic: &[AutoLanguagePreference],
) -> Vec<String> {
  let automatic_by_base = automatic
    .iter()
    .map(|language| (language.base.clone(), language.request.clone()))
    .collect::<HashMap<_, _>>();
  let mut resolved = Vec::new();
  let mut seen = HashSet::new();

  for language in requested {
    let Some(candidate) = automatic_by_base
      .get(&subtitle_language_base(language))
      .cloned()
    else {
      continue;
    };

    if seen.insert(candidate.clone()) {
      resolved.push(candidate);
    }
  }

  resolved
}

fn preferred_auto_languages(automatic: &[String]) -> Vec<AutoLanguagePreference> {
  let mut grouped = HashMap::<String, (Option<String>, Option<String>)>::new();

  for language in automatic {
    let base = subtitle_language_base(language);
    let entry = grouped.entry(base).or_insert((None, None));
    if language.ends_with("-orig") {
      entry.1 = Some(language.clone());
    } else {
      entry.0 = Some(language.clone());
    }
  }

  let mut preferred = grouped
    .into_iter()
    .map(|(base, (plain, orig))| AutoLanguagePreference {
      request: plain
        .or(orig)
        .expect("auto language group should not be empty"),
      base,
    })
    .collect::<Vec<_>>();
  preferred.sort_by(|left, right| left.base.cmp(&right.base));
  preferred
}

fn subtitle_language_base(language: &str) -> String {
  language
    .trim()
    .trim_end_matches("-orig")
    .to_ascii_lowercase()
}

fn subtitle_language_bases(languages: &[String]) -> Vec<String> {
  let mut seen = HashSet::new();
  let mut bases = Vec::new();

  for language in languages {
    let base = subtitle_language_base(language);
    if seen.insert(base.clone()) {
      bases.push(base);
    }
  }

  bases
}

#[cfg(test)]
mod tests {
  #[tokio::test]
  async fn slow_reaping_does_not_finish_shutdown_before_confirmation() {
    use super::{mpsc, Duration, YtdlpChild};

    let (shutdown, shutdown_rx) = mpsc::channel();
    let (reaped_tx, reaped) = tokio::sync::oneshot::channel();
    let mut child = YtdlpChild { shutdown, reaped };
    let mut stopped = std::pin::pin!(child.shutdown());
    assert!(
      tokio::time::timeout(Duration::from_millis(3200), stopped.as_mut())
        .await
        .is_err()
    );
    assert!(shutdown_rx.try_recv().is_ok());
    reaped_tx.send(()).unwrap();
    stopped.await.unwrap();
  }

  #[tokio::test]
  async fn shutdown_reports_lost_reaping_confirmation() {
    use super::{mpsc, YtdlpChild};

    let (shutdown, shutdown_rx) = mpsc::channel();
    let (reaped_tx, reaped) = tokio::sync::oneshot::channel();
    let mut child = YtdlpChild { shutdown, reaped };
    drop(reaped_tx);

    assert!(child.shutdown().await.is_err());
    assert!(shutdown_rx.try_recv().is_ok());
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn cancellation_reaps_term_resistant_group() {
    use super::{spawn_download_command, Command, Duration, YtdlpCommandEvent};

    let mut command = Command::new("/bin/sh");
    command.args(["-c", "trap '' TERM; sleep 30 & echo ready; wait"]);
    let (mut events, mut child, pid) = spawn_download_command(command).unwrap();
    let ready = tokio::time::timeout(Duration::from_secs(3), events.recv())
      .await
      .expect("shell did not install its signal handler");
    assert!(matches!(ready, Some(YtdlpCommandEvent::Stdout(line)) if line == b"ready"));

    child.shutdown().await.unwrap();
    let mut status = 0;
    assert_eq!(
      unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
      -1
    );
    assert_eq!(
      std::io::Error::last_os_error().raw_os_error(),
      Some(libc::ECHILD)
    );
    tokio::time::timeout(Duration::from_secs(3), async {
      while events.recv().await.is_some() {}
    })
    .await
    .expect("a descendant still owns the process pipes");
  }

  use super::{
    build_sponsorblock_args, build_subtitle_args, normalize_extractor_args, summarize_args_for_log,
  };
  use crate::models::SubtitleInventory;
  use crate::state::config_models::{SponsorBlockSettings, SubtitleSettings};

  fn inventory(manual: &[&str], automatic: &[&str]) -> SubtitleInventory {
    SubtitleInventory {
      manual_languages: manual.iter().map(|value| (*value).to_string()).collect(),
      auto_languages: automatic.iter().map(|value| (*value).to_string()).collect(),
    }
  }

  #[test]
  fn chinese_priority_falls_back_to_traditional_and_keeps_ids() {
    let settings = SubtitleSettings::default();
    let args = build_subtitle_args(
      &settings,
      None,
      Some(&inventory(&["zh-Hant"], &["en-orig", "zh-Hans"])),
    )
    .expect("Chinese subtitle");
    assert_eq!(args.last().unwrap(), r"^zh\-Hant$");
    assert!(args
      .windows(2)
      .any(|pair| pair == ["--convert-subs", "srt"]));
    assert!(!args.iter().any(|arg| arg == "--embed-subs"));
  }

  #[test]
  fn chinese_priority_accepts_source_suffixed_translation() {
    let args = build_subtitle_args(
      &SubtitleSettings::default(),
      None,
      Some(&inventory(&["en"], &["en", "zh-Hans-en", "zh-Hant-en"])),
    )
    .expect("translated Chinese subtitle");
    assert_eq!(args.last().unwrap(), r"^zh\-Hans\-en$");
  }

  #[test]
  fn chinese_priority_prefers_existing_traditional_over_suffixed_translation() {
    let args = build_subtitle_args(
      &SubtitleSettings::default(),
      None,
      Some(&inventory(&["zh-Hant"], &["zh-Hans-en"])),
    )
    .unwrap();
    assert_eq!(args.last().unwrap(), r"^zh\-Hant$");
  }

  #[test]
  fn chinese_priority_accepts_traditional_source_suffixed_translation() {
    let args = build_subtitle_args(
      &SubtitleSettings::default(),
      None,
      Some(&inventory(&[], &["zh-Hant-en"])),
    )
    .unwrap();
    assert_eq!(args.last().unwrap(), r"^zh\-Hant\-en$");
  }

  #[test]
  fn chinese_priority_keeps_translation_fallback() {
    let args = build_subtitle_args(
      &SubtitleSettings::default(),
      None,
      Some(&inventory(&[], &["en-orig", "zh-Hans"])),
    )
    .expect("translation");
    assert_eq!(args.last().unwrap(), r"^zh\-Hans$");
  }

  #[test]
  fn subtitle_regression_explicit_auto_preserves_orig_selection() {
    let overrides = crate::models::download::SubtitleOverrides {
      auto_languages: Some(vec![" EN-ORIG ".into()]),
      ..Default::default()
    };
    let args = build_subtitle_args(
      &SubtitleSettings::default(),
      Some(&overrides),
      Some(&inventory(&[], &["en", "en-orig"])),
    )
    .unwrap();
    assert_eq!(args.last().unwrap(), r"^en\-orig$");
  }

  #[test]
  fn subtitle_regression_explicit_manual_preserves_orig_selection() {
    let overrides = crate::models::download::SubtitleOverrides {
      manual_languages: Some(vec![" EN-ORIG ".into()]),
      ..Default::default()
    };
    let args = build_subtitle_args(
      &SubtitleSettings::default(),
      Some(&overrides),
      Some(&inventory(&["en", "en-orig"], &[])),
    )
    .unwrap();
    assert_eq!(args.last().unwrap(), r"^en\-orig$");
  }

  #[test]
  fn subtitle_regression_explicit_ids_do_not_fall_back_to_base() {
    for automatic in [false, true] {
      let overrides = crate::models::download::SubtitleOverrides {
        manual_languages: (!automatic).then(|| vec!["en".into()]),
        auto_languages: automatic.then(|| vec!["en".into()]),
        ..Default::default()
      };
      assert!(build_subtitle_args(
        &SubtitleSettings::default(),
        Some(&overrides),
        Some(&inventory(&["en-orig"], &["en-orig"]))
      )
      .is_none());
    }
  }

  #[test]
  fn subtitle_regression_explicit_all_keeps_both_auto_ids() {
    let overrides = crate::models::download::SubtitleOverrides {
      auto_languages: Some(vec!["all".into()]),
      ..Default::default()
    };
    let args = build_subtitle_args(
      &SubtitleSettings::default(),
      Some(&overrides),
      Some(&inventory(&[], &["en", "en-orig", "zh-Hans"])),
    )
    .unwrap();
    assert_eq!(args.last().unwrap(), r"^en$,^en\-orig$,^zh\-Hans$");
  }

  #[test]
  fn subtitle_regression_empty_explicit_source_does_not_select_english() {
    let overrides = crate::models::download::SubtitleOverrides {
      manual_languages: Some(vec![]),
      auto_languages: Some(vec!["zh-Hans".into()]),
      ..Default::default()
    };
    let args = build_subtitle_args(
      &SubtitleSettings::default(),
      Some(&overrides),
      Some(&inventory(&["en"], &["zh-Hans"])),
    )
    .unwrap();
    assert_eq!(args.last().unwrap(), r"^zh\-Hans$");
  }

  #[test]
  fn subtitle_regression_explicit_case_insensitive_keeps_platform_case() {
    let overrides = crate::models::download::SubtitleOverrides {
      manual_languages: Some(vec![" zh-hans ".into()]),
      auto_languages: Some(vec![" ZH-HANT ".into()]),
      ..Default::default()
    };
    let args = build_subtitle_args(
      &SubtitleSettings::default(),
      Some(&overrides),
      Some(&inventory(&["zh-Hans"], &["zh-Hant"])),
    )
    .unwrap();
    assert_eq!(args.last().unwrap(), r"^zh\-Hans$,^zh\-Hant$");
  }

  #[test]
  fn subtitles_disabled_returns_none() {
    let settings = SubtitleSettings {
      enabled: false,
      ..Default::default()
    };
    assert!(build_subtitle_args(&settings, None, Some(&inventory(&["en"], &[]))).is_none());
  }

  #[test]
  fn subtitles_enabled_uses_defaults_when_available() {
    let mut settings = SubtitleSettings {
      enabled: true,
      chinese_priority: false,
      languages: vec!["en".into()],
      ..Default::default()
    };
    settings.languages.clear();
    settings.format_preference.clear();

    let args = build_subtitle_args(&settings, None, Some(&inventory(&["en"], &[]))).expect("args");
    assert_eq!(
      args,
      vec![
        "--write-subs",
        "--no-embed-subs",
        "--convert-subs",
        "srt",
        "--sub-format",
        "srt/vtt/ass/ttml/json3",
        "--sub-langs",
        "^en$"
      ]
    );
  }

  #[test]
  fn subtitles_embed_and_include_auto_generated_enabled() {
    let settings = SubtitleSettings {
      enabled: true,
      embed_subtitles: true,
      include_auto_generated: true,
      chinese_priority: false,
      languages: vec!["en".into()],
      ..Default::default()
    };
    let args = build_subtitle_args(&settings, None, Some(&inventory(&[], &["en"]))).expect("args");
    assert_eq!(
      args,
      vec![
        "--write-subs",
        "--no-embed-subs",
        "--convert-subs",
        "srt",
        "--write-auto-subs",
        "--sub-format",
        "srt/vtt/ass/ttml/json3",
        "--sub-langs",
        "^en$"
      ]
    );
  }

  #[test]
  fn subtitles_enabled_honors_all_language() {
    let settings = SubtitleSettings {
      enabled: true,
      languages: vec!["all".into(), "en".into()],
      format_preference: vec!["srt".into(), "vtt".into()],
      embed_subtitles: false,
      chinese_priority: false,
      include_auto_generated: true,
    };

    let args =
      build_subtitle_args(&settings, None, Some(&inventory(&["de"], &["en"]))).expect("args");
    assert_eq!(
      args,
      vec![
        "--write-subs",
        "--no-embed-subs",
        "--convert-subs",
        "srt",
        "--sub-format",
        "srt/vtt/ass/ttml/json3",
        "--sub-langs",
        "^de$"
      ]
    );
  }

  #[test]
  fn subtitles_trim_and_dedupe_values() {
    let settings = SubtitleSettings {
      enabled: true,
      languages: vec![" en ".into(), "EN".into(), "pt-BR".into(), String::new()],
      ..Default::default()
    };
    let args =
      build_subtitle_args(&settings, None, Some(&inventory(&["en", "pt-br"], &[]))).expect("args");
    assert_eq!(args[7], r"^en$,^pt\-br$");
  }

  #[test]
  fn subtitles_normalize_legacy_json_format_to_json3() {
    let settings = SubtitleSettings {
      enabled: true,
      format_preference: vec!["json".into(), "json3".into()],
      languages: vec!["en".into()],
      ..Default::default()
    };

    let args = build_subtitle_args(&settings, None, Some(&inventory(&["en"], &[]))).expect("args");
    assert_eq!(args[5], "json3/srt/vtt/ass/ttml");
  }

  #[test]
  fn subtitles_skip_unavailable_requested_languages() {
    let settings = SubtitleSettings {
      enabled: true,
      languages: vec!["fr".into()],
      ..Default::default()
    };

    assert!(build_subtitle_args(&settings, None, Some(&inventory(&["en"], &[]))).is_none());
  }

  #[test]
  fn subtitles_skip_auto_languages_when_auto_generated_disabled() {
    let settings = SubtitleSettings {
      enabled: true,
      include_auto_generated: false,
      ..Default::default()
    };

    assert!(build_subtitle_args(&settings, None, Some(&inventory(&[], &["en"]))).is_none());
  }

  #[test]
  fn subtitles_include_auto_when_manual_and_auto_languages_match() {
    let settings = SubtitleSettings {
      enabled: true,
      include_auto_generated: true,
      chinese_priority: false,
      languages: vec!["en".into()],
      ..Default::default()
    };

    let args =
      build_subtitle_args(&settings, None, Some(&inventory(&["en"], &["en"]))).expect("args");
    assert_eq!(args[4], "--write-auto-subs");
    assert_eq!(args[8], "^en$");
  }

  #[test]
  fn subtitles_match_base_language_to_orig_auto_caption() {
    let settings = SubtitleSettings {
      enabled: true,
      include_auto_generated: true,
      chinese_priority: false,
      languages: vec!["en".into()],
      ..Default::default()
    };

    let args =
      build_subtitle_args(&settings, None, Some(&inventory(&[], &["en-orig"]))).expect("args");
    assert_eq!(args[4], "--write-auto-subs");
    assert_eq!(args[8], r"^en\-orig$");
  }

  #[test]
  fn subtitles_prefer_plain_auto_caption_when_plain_and_orig_exist() {
    let settings = SubtitleSettings {
      enabled: true,
      include_auto_generated: true,
      chinese_priority: false,
      languages: vec!["en".into()],
      ..Default::default()
    };

    let args = build_subtitle_args(&settings, None, Some(&inventory(&[], &["en", "en-orig"])))
      .expect("args");
    assert_eq!(args[4], "--write-auto-subs");
    assert_eq!(args[8], "^en$");
  }

  #[test]
  fn subtitles_ignore_translation_only_auto_captions_when_orig_variants_exist() {
    let settings = SubtitleSettings {
      enabled: true,
      include_auto_generated: true,
      chinese_priority: false,
      languages: vec!["en".into()],
      ..Default::default()
    };

    let args = build_subtitle_args(
      &settings,
      None,
      Some(&inventory(&[], &["en", "en-orig", "de", "fr"])),
    )
    .expect("args");
    assert_eq!(args[4], "--write-auto-subs");
    assert_eq!(args[8], "^en$");
  }

  #[test]
  fn subtitles_require_inventory() {
    let settings = SubtitleSettings {
      enabled: true,
      chinese_priority: false,
      languages: vec!["en".into()],
      ..Default::default()
    };

    assert!(build_subtitle_args(&settings, None, None).is_none());
  }

  #[test]
  fn subtitles_source_specific_override_allows_independent_manual_and_auto_selection() {
    let settings = SubtitleSettings {
      enabled: true,
      chinese_priority: false,
      languages: vec!["en".into()],
      ..Default::default()
    };
    let overrides = crate::models::download::SubtitleOverrides {
      manual_languages: Some(vec!["en".into()]),
      auto_languages: Some(vec!["nl".into()]),
      ..Default::default()
    };

    let args = build_subtitle_args(
      &settings,
      Some(&overrides),
      Some(&inventory(&["en"], &["nl"])),
    )
    .expect("args");
    assert_eq!(args[4], "--write-auto-subs");
    assert_eq!(args[8], "^en$,^nl$");
  }

  #[test]
  fn extractor_args_empty_returns_none() {
    assert_eq!(normalize_extractor_args(""), None);
    assert_eq!(normalize_extractor_args("   "), None);
  }

  #[test]
  fn extractor_args_trim_whitespace() {
    assert_eq!(
      normalize_extractor_args(" youtube:player_js_variant=main "),
      Some("youtube:player_js_variant=main".into())
    );
  }

  #[test]
  fn extractor_args_join_non_empty_lines_with_spaces() {
    assert_eq!(
      normalize_extractor_args(
        " youtube:player_js_variant=main \n\n youtube:skip=hls,dash \r\n generic:impersonate"
      ),
      Some("youtube:player_js_variant=main youtube:skip=hls,dash generic:impersonate".into())
    );
  }

  #[test]
  fn sponsorblock_remove_omits_force_keyframes_at_cuts_by_default() {
    let settings = SponsorBlockSettings {
      remove_parts: vec!["sponsor".into(), "intro".into()],
      ..Default::default()
    };

    assert_eq!(
      build_sponsorblock_args(&settings, false),
      vec!["--sponsorblock-remove", "sponsor,intro"]
    );
  }

  #[test]
  fn sponsorblock_remove_precise_cuts_add_force_keyframes_at_cuts() {
    let settings = SponsorBlockSettings {
      remove_parts: vec!["sponsor".into(), "intro".into()],
      ..Default::default()
    };

    assert_eq!(
      build_sponsorblock_args(&settings, true),
      vec![
        "--sponsorblock-remove",
        "sponsor,intro",
        "--force-keyframes-at-cuts"
      ]
    );
  }

  #[test]
  fn sponsorblock_mark_does_not_add_force_keyframes_at_cuts() {
    let settings = SponsorBlockSettings {
      mark_parts: vec!["sponsor".into()],
      ..Default::default()
    };

    assert_eq!(
      build_sponsorblock_args(&settings, false),
      vec!["--sponsorblock-mark", "sponsor"]
    );
  }

  #[test]
  fn summary_flags_presence_regardless_of_value_shape() {
    // Includes a secret value that itself starts with '-', which the old
    // peek-based redaction logic could fail to redact (it assumed any
    // token starting with '-' was the next flag, not a value). The
    // presence-only approach never looks at the value at all, so this
    // shape can no longer cause a leak.
    let args = vec![
      "--username".into(),
      "my_secure_user".into(),
      "--password".into(),
      "-secret_starting_with_dash".into(),
      "--proxy".into(),
      "http://user:pass@proxy.example.com:8080".into(),
      "--cookies-from-browser".into(),
      "chrome".into(),
      "--url".into(),
      "https://youtube.com/watch?v=dQw4w9WgXcQ".into(),
      "--no-playlist".into(),
    ];

    let summary = summarize_args_for_log(&args);

    assert_eq!(summary.arg_count, args.len());
    assert!(summary.has_auth);
    assert!(summary.has_proxy);
    assert!(summary.has_browser_cookies);
    // No plain "--cookies" flag was passed (only "--cookies-from-browser").
    assert!(!summary.has_cookies);
  }

  #[test]
  fn summary_detects_equals_form_flags() {
    let args = vec![
      "--proxy=http://proxy.example.com:8080".into(),
      "--cookies=/home/user/.config/cookies.txt".into(),
      "--add-header=Authorization:Bearer eyJhbGciOiJIUzI1NiIs...".into(),
    ];

    let summary = summarize_args_for_log(&args);

    assert!(summary.has_proxy);
    assert!(summary.has_cookies);
    assert!(summary.has_auth);
    assert!(!summary.has_browser_cookies);
  }

  #[test]
  fn summary_all_false_when_no_sensitive_flags_present() {
    let args = vec![
      "--url".into(),
      "https://youtube.com/watch?v=dQw4w9WgXcQ".into(),
      "--no-playlist".into(),
    ];

    let summary = summarize_args_for_log(&args);

    assert_eq!(summary.arg_count, 3);
    assert!(!summary.has_proxy);
    assert!(!summary.has_cookies);
    assert!(!summary.has_browser_cookies);
    assert!(!summary.has_auth);
  }
}
