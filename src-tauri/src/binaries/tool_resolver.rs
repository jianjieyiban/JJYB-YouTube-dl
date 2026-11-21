use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use crate::paths::PathsManager;

pub const REQUIRED_TOOLS: [&str; 4] = ["yt-dlp", "ffmpeg", "ffprobe", "deno"];

#[derive(Clone)]
pub struct ToolResolver {
  dirs: Vec<PathBuf>,
  probe_roots: ProbeRoots,
}

#[derive(Clone)]
struct ProbeRoots {
  app_dir: PathBuf,
  tmp_dir: PathBuf,
  cache_dir: PathBuf,
}

impl ProbeRoots {
  fn configure(&self, command: &mut std::process::Command) -> std::io::Result<()> {
    for dir in [
      &self.app_dir,
      &self.tmp_dir,
      &self.cache_dir.join("deno"),
      &self.cache_dir.join("python"),
    ] {
      std::fs::create_dir_all(dir)?;
    }
    command
      .current_dir(&self.app_dir)
      .env("TEMP", &self.tmp_dir)
      .env("TMP", &self.tmp_dir)
      .env("TMPDIR", &self.tmp_dir)
      .env("DENO_DIR", self.cache_dir.join("deno"))
      .env("PYTHONDONTWRITEBYTECODE", "1")
      .env("PYTHONPYCACHEPREFIX", self.cache_dir.join("python"));
    Ok(())
  }
}

impl ToolResolver {
  pub fn new(paths: &PathsManager) -> Self {
    Self {
      dirs: paths.tool_dirs(),
      probe_roots: ProbeRoots {
        app_dir: paths.app_dir().clone(),
        tmp_dir: paths.tmp_dir(),
        cache_dir: paths.cache_dir(),
      },
    }
  }

  pub fn resolve(&self, tool: &str) -> Option<PathBuf> {
    self.resolve_with(tool, |path, _| self.probe(path, tool))
  }

  pub fn tool_dirs(&self) -> &[PathBuf] {
    &self.dirs
  }

  pub fn ffmpeg_pair(&self) -> (Option<PathBuf>, Option<PathBuf>) {
    let ffmpeg = self.resolve("ffmpeg");
    if let Some(ref binary) = ffmpeg {
      let sibling = binary.parent().unwrap().join(binary_name("ffprobe"));
      if self.probe(&sibling, "ffprobe") {
        return (ffmpeg, Some(sibling));
      }
    }
    for dir in &self.dirs {
      let encoder = dir.join(binary_name("ffmpeg"));
      let decoder = dir.join(binary_name("ffprobe"));
      if self.probe(&encoder, "ffmpeg") && self.probe(&decoder, "ffprobe") {
        return (Some(encoder), Some(decoder));
      }
    }
    (ffmpeg, self.resolve("ffprobe"))
  }

  fn probe(&self, path: &Path, tool: &str) -> bool {
    probe_scoped(path, tool, Duration::from_secs(5), Some(&self.probe_roots))
  }

  fn resolve_with(
    &self,
    tool: &str,
    mut validate: impl FnMut(&Path, &str) -> bool,
  ) -> Option<PathBuf> {
    let arg = version_arg(tool)?;
    self
      .dirs
      .iter()
      .map(|dir| dir.join(binary_name(tool)))
      .find(|path| validate(path, arg))
  }
}

pub fn binary_name(tool: &str) -> String {
  format!("{tool}{}", std::env::consts::EXE_SUFFIX)
}

fn version_arg(tool: &str) -> Option<&'static str> {
  match tool {
    "ffmpeg" | "ffprobe" => Some("-version"),
    "yt-dlp" | "deno" => Some("--version"),
    _ => None,
  }
}

#[derive(Debug)]
enum ProbeFailure {
  Runtime(Option<std::io::Error>),
  Spawn(std::io::Error),
  Io(std::io::Error),
  Timeout,
  Exit(std::process::ExitStatus),
  Size { stdout: usize, stderr: usize },
  Version,
}

impl ProbeFailure {
  fn category(&self) -> &'static str {
    match self {
      Self::Runtime(_) => "runtime",
      Self::Spawn(_) => "spawn",
      Self::Io(_) => "io",
      Self::Timeout => "timeout",
      Self::Exit(_) => "exit",
      Self::Size { .. } => "size",
      Self::Version => "version",
    }
  }
}

#[cfg(test)]
fn probe(path: &Path, tool: &str, timeout: Duration) -> bool {
  probe_scoped(path, tool, timeout, None)
}

fn probe_scoped(path: &Path, tool: &str, timeout: Duration, roots: Option<&ProbeRoots>) -> bool {
  if version_arg(tool).is_none() || !path.is_absolute() || !path.is_file() {
    return false;
  }
  let started = std::time::Instant::now();
  let result = probe_result_scoped(path, tool, timeout, roots);
  let elapsed_ms = started.elapsed().as_millis() as u64;
  match &result {
    Ok(()) => {
      tracing::debug!(target: "jjyb::tool_probe", tool, path = %path.display(), elapsed_ms, category = "success", "Tool version probe succeeded")
    }
    Err(failure) => {
      let (io_kind, os_error) = match failure {
        ProbeFailure::Runtime(Some(error))
        | ProbeFailure::Spawn(error)
        | ProbeFailure::Io(error) => (Some(error.kind()), error.raw_os_error()),
        _ => (None, None),
      };
      let exit_code = match failure {
        ProbeFailure::Exit(status) => status.code(),
        _ => None,
      };
      let (stdout_bytes, stderr_bytes) = match failure {
        ProbeFailure::Size { stdout, stderr } => (Some(*stdout), Some(*stderr)),
        _ => (None, None),
      };
      tracing::warn!(target: "jjyb::tool_probe", tool, path = %path.display(), elapsed_ms,
        category = failure.category(), ?io_kind, ?os_error, ?exit_code, ?stdout_bytes, ?stderr_bytes,
        "Tool version probe failed");
    }
  }
  result.is_ok()
}

#[cfg(test)]
fn probe_result(path: &Path, tool: &str, timeout: Duration) -> Result<(), ProbeFailure> {
  probe_result_scoped(path, tool, timeout, None)
}

fn probe_result_scoped(
  path: &Path,
  tool: &str,
  timeout: Duration,
  roots: Option<&ProbeRoots>,
) -> Result<(), ProbeFailure> {
  let arg = version_arg(tool).ok_or(ProbeFailure::Version)?;
  std::thread::scope(|scope| {
    scope
      .spawn(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
          .enable_all()
          .build()
          .map_err(|error| ProbeFailure::Runtime(Some(error)))?;
        runtime.block_on(async {
          use tokio::io::AsyncReadExt;
          const LIMIT: u64 = 64 * 1024;
          let mut base_command = std::process::Command::new(path);
          if let Some(roots) = roots {
            roots
              .configure(&mut base_command)
              .map_err(ProbeFailure::Io)?;
          }
          let mut command = tokio::process::Command::from(base_command);
          command
            .arg(arg)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
          #[cfg(windows)]
          command.creation_flags(0x08000000);
          let mut child = command.spawn().map_err(ProbeFailure::Spawn)?;
          let mut stdout = child.stdout.take().unwrap().take(LIMIT + 1);
          let mut stderr = child.stderr.take().unwrap().take(LIMIT + 1);
          let output = tokio::time::timeout(timeout, async {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let (status, _, _) = tokio::try_join!(
              child.wait(),
              stdout.read_to_end(&mut out),
              stderr.read_to_end(&mut err),
            )?;
            Ok::<_, std::io::Error>((status, out, err))
          })
          .await;
          let (status, out, err) = match output {
            Ok(Ok(output)) => output,
            failure => {
              let _ = child.start_kill();
              return Err(match failure {
                Ok(Err(error)) => ProbeFailure::Io(error),
                Err(_) => ProbeFailure::Timeout,
                Ok(Ok(_)) => unreachable!(),
              });
            }
          };
          if !status.success() {
            return Err(ProbeFailure::Exit(status));
          }
          if out.len() > LIMIT as usize || err.len() > LIMIT as usize {
            return Err(ProbeFailure::Size {
              stdout: out.len(),
              stderr: err.len(),
            });
          }
          if !valid_version(tool, &out) && !valid_version(tool, &err) {
            return Err(ProbeFailure::Version);
          }
          Ok(())
        })
      })
      .join()
      .unwrap_or(Err(ProbeFailure::Runtime(None)))
  })
}

fn valid_version(tool: &str, output: &[u8]) -> bool {
  let Ok(text) = std::str::from_utf8(output) else {
    return false;
  };
  let Some(line) = text.lines().next().map(str::trim) else {
    return false;
  };
  let pattern = match tool {
    "ffmpeg" => r"^ffmpeg version [nN]?[0-9][^\s]*",
    "ffprobe" => r"^ffprobe version [nN]?[0-9][^\s]*",
    "deno" => r"^deno [0-9]+\.[0-9]+\.[0-9]+(?:[-+\s]|$)",
    "yt-dlp" => r"^[0-9]{4}\.[0-9]{2}\.[0-9]{2}(?:\.[0-9]+)?$",
    _ => return false,
  };
  regex::Regex::new(pattern).unwrap().is_match(line)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::process::Command;
  use std::time::Instant;

  #[test]
  fn probe_command_contains_temp_and_cache_under_its_own_root() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("../../../.task-tmp")
      .join(format!("tool-probe-{}", std::process::id()));
    let roots = ProbeRoots {
      app_dir: root.clone(),
      tmp_dir: root.join("tmp"),
      cache_dir: root.join("cache"),
    };
    let mut command = Command::new("deno");
    roots.configure(&mut command).unwrap();
    assert_eq!(command.get_current_dir(), Some(root.as_path()));
    let env = command
      .get_envs()
      .collect::<std::collections::HashMap<_, _>>();
    for key in ["TEMP", "TMP", "TMPDIR"] {
      assert_eq!(
        env[std::ffi::OsStr::new(key)],
        Some(roots.tmp_dir.as_os_str())
      );
    }
    let deno = roots.cache_dir.join("deno");
    let python = roots.cache_dir.join("python");
    assert_eq!(
      env[std::ffi::OsStr::new("DENO_DIR")],
      Some(deno.as_os_str())
    );
    assert_eq!(
      env[std::ffi::OsStr::new("PYTHONPYCACHEPREFIX")],
      Some(python.as_os_str())
    );
    assert_eq!(
      env[std::ffi::OsStr::new("PYTHONDONTWRITEBYTECODE")],
      Some(std::ffi::OsStr::new("1"))
    );
    assert!(roots.tmp_dir.is_dir());
    assert!(deno.is_dir());
    assert!(python.is_dir());
    let other = ProbeRoots {
      app_dir: root.join("other"),
      tmp_dir: root.join("other/tmp"),
      cache_dir: root.join("other/cache"),
    };
    let mut other_command = Command::new("deno");
    other.configure(&mut other_command).unwrap();
    assert_eq!(
      other_command.get_current_dir(),
      Some(other.app_dir.as_path())
    );
    assert_eq!(command.get_current_dir(), Some(root.as_path()));
    assert_eq!(
      command
        .get_envs()
        .collect::<std::collections::HashMap<_, _>>(),
      env
    );
    std::fs::remove_dir_all(&roots.tmp_dir).unwrap();
    std::fs::write(&roots.tmp_dir, b"keep").unwrap();
    assert!(roots.configure(&mut Command::new("deno")).is_err());
    assert_eq!(std::fs::read(&roots.tmp_dir).unwrap(), b"keep");
    std::fs::remove_dir_all(root).unwrap();
  }

  #[test]
  fn skips_invalid_candidates_and_uses_correct_version_flag() {
    let root = std::env::temp_dir();
    let dirs = vec![
      root.join("portable"),
      root.join("managed"),
      root.join("path"),
    ];
    let resolver = ToolResolver {
      dirs: dirs.clone(),
      probe_roots: ProbeRoots {
        app_dir: root.clone(),
        tmp_dir: root.join("tmp"),
        cache_dir: root.join("cache"),
      },
    };
    let mut checked = Vec::new();
    let found = resolver.resolve_with("ffmpeg", |path, arg| {
      assert_eq!(arg, "-version");
      checked.push(path.to_path_buf());
      path.parent() == Some(dirs[2].as_path())
    });
    assert_eq!(found, Some(dirs[2].join(binary_name("ffmpeg"))));
    assert_eq!(checked.len(), 3);
    assert_eq!(version_arg("yt-dlp"), Some("--version"));
    assert_eq!(version_arg("deno"), Some("--version"));
    assert_eq!(version_arg("ffprobe"), Some("-version"));
  }

  #[test]
  fn version_probe_kills_timed_out_process_and_rejects_failure() {
    let (_dir, executable) =
      fixture("fn main() { std::thread::sleep(std::time::Duration::from_secs(10)); }");
    let start = Instant::now();
    assert!(!probe(&executable, "deno", Duration::from_millis(100)));
    assert!(start.elapsed() < Duration::from_secs(3));
    let (_dir, executable) =
      fixture("fn main() { println!(\"deno 2.5.6\"); std::process::exit(1); }");
    assert!(!probe(&executable, "deno", Duration::from_secs(5)));
    assert!(!probe(
      &executable.with_extension("missing"),
      "deno",
      Duration::from_secs(1)
    ));
  }

  #[test]
  fn probe_failures_are_classified_without_output_contents() {
    for (source, expected) in [
      (
        "fn main() { std::thread::sleep(std::time::Duration::from_secs(10)); }",
        "timeout",
      ),
      (
        "fn main() { println!(\"deno 2.5.6\"); std::process::exit(7); }",
        "exit",
      ),
      ("fn main() { println!(\"not a version\"); }", "version"),
      ("fn main() { print!(\"{}\", \"x\".repeat(65537)); }", "size"),
      (
        "fn main() { eprint!(\"{}\", \"x\".repeat(65537)); }",
        "size",
      ),
    ] {
      let (_dir, executable) = fixture(source);
      let timeout = if expected == "timeout" {
        Duration::from_millis(200)
      } else {
        Duration::from_secs(5)
      };
      let failure = probe_result(&executable, "deno", timeout).unwrap_err();
      assert_eq!(failure.category(), expected);
    }
    let dir = tempfile::tempdir().unwrap();
    let invalid = dir.path().join(binary_name("invalid"));
    std::fs::write(&invalid, b"not an executable").unwrap();
    assert_eq!(
      probe_result(&invalid, "deno", Duration::from_secs(5))
        .unwrap_err()
        .category(),
      "spawn"
    );
    assert_eq!(
      ProbeFailure::Runtime(Some(std::io::Error::other("not logged"))).category(),
      "runtime"
    );
    assert_eq!(
      ProbeFailure::Io(std::io::Error::other("not logged")).category(),
      "io"
    );
  }

  #[test]
  fn probe_logs_only_existing_candidates_without_process_output() {
    const ISOLATED: &str = "JJYB_ISOLATED_PROBE_LOG_TEST";
    if std::env::var_os(ISOLATED).is_none() {
      assert!(Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "binaries::tool_resolver::tests::probe_logs_only_existing_candidates_without_process_output", "--nocapture"])
        .env(ISOLATED, "1")
        .status()
        .unwrap()
        .success());
      return;
    }
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct Writer(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Writer {
      fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
      }
      fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
      }
    }
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer = Writer(buffer.clone());
    let subscriber = tracing_subscriber::fmt()
      .without_time()
      .with_ansi(false)
      .with_writer(move || writer.clone())
      .with_max_level(tracing::Level::INFO)
      .finish();
    let (_dir, executable) =
      fixture("fn main() { println!(\"private stdout\"); eprintln!(\"private stderr\"); }");
    tracing::subscriber::with_default(subscriber, || {
      assert!(!probe(
        &executable.with_extension("missing"),
        "deno",
        Duration::from_secs(5)
      ));
      assert!(buffer.lock().unwrap().is_empty());
      assert!(!probe(&executable, "deno", Duration::from_secs(5)));
    });
    let log = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    assert!(log.contains("jjyb::tool_probe"));
    assert!(log.contains("category=\"version\""));
    assert!(log.contains("elapsed_ms="));
    assert!(log.contains(&executable.display().to_string()));
    assert!(!log.contains("private stdout"));
    assert!(!log.contains("private stderr"));
  }

  #[test]
  fn version_validation_remains_strict_on_first_line() {
    assert!(!valid_version("deno", b"unrelated\ndeno 2.5.6"));
    assert!(!valid_version("yt-dlp", b"2026.11.12 trailing"));
    assert!(!valid_version("deno", b"deno 2.5.6\n\xff"));
    assert!(valid_version("yt-dlp", b"2026.11.12\nadditional line"));
  }

  fn fixture(source: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("fixture.rs");
    let executable = dir.path().join(binary_name("fixture"));
    std::fs::write(&source_path, source).unwrap();
    assert!(Command::new("rustc")
      .arg(&source_path)
      .arg("-o")
      .arg(&executable)
      .status()
      .unwrap()
      .success());
    (dir, executable)
  }

  #[test]
  fn version_probe_rejects_success_without_tool_identity() {
    let (_dir, executable) = fixture("fn main() { println!(\"unrelated program 1.0\"); }");
    for tool in REQUIRED_TOOLS {
      assert!(!probe(&executable, tool, Duration::from_secs(5)), "{tool}");
    }
    assert!(!probe(
      &std::env::current_exe().unwrap(),
      "--list",
      Duration::from_secs(5)
    ));
  }

  #[test]
  fn version_probe_accepts_only_matching_real_version_output() {
    for (tool, output) in [
      ("ffmpeg", "ffmpeg version 7.1 Copyright (c)"),
      ("ffprobe", "ffprobe version n7.1-12-g123 Copyright (c)"),
      (
        "deno",
        "deno 2.5.6 (stable, release, x86_64-pc-windows-msvc)\\nv8 14.0",
      ),
      ("yt-dlp", "2026.11.12.232913"),
    ] {
      let flag = version_arg(tool).unwrap();
      let source = format!("fn main() {{ assert_eq!(std::env::args().nth(1).unwrap(), {flag:?}); println!({output:?}); }}");
      let (_dir, executable) = fixture(&source);
      assert!(probe(&executable, tool, Duration::from_secs(5)), "{tool}");
      for other in REQUIRED_TOOLS.into_iter().filter(|other| *other != tool) {
        assert!(!probe(&executable, other, Duration::from_secs(5)));
      }
    }
  }

  #[test]
  fn version_probe_bounds_output_and_wait() {
    let (_dir, executable) = fixture("fn main() { loop { println!(\"ffmpeg version 7.1\"); } }");
    let start = Instant::now();
    assert!(!probe(&executable, "ffmpeg", Duration::from_millis(200)));
    assert!(start.elapsed() < Duration::from_secs(3));
  }

  #[test]
  fn explicit_updates_and_optional_tools_remain_connected() {
    let store = include_str!("../../../src/stores/binaries.ts");
    let settings = include_str!("../../../src/components/settings/SettingsUpdate.vue");
    let install = include_str!("../../../src/views/full/InstallView.vue");
    assert!(store.contains("'binaries_check_updates'"));
    assert!(store.contains("'binaries_update'"));
    assert!(settings.contains("binariesStore.checkUpdates()"));
    assert!(install.contains("binariesStore.update()"));
    let runner = include_str!("../runners/ytdlp_runner.rs");
    assert!(runner.contains("self.resolver.tool_dirs()"));
  }
}
