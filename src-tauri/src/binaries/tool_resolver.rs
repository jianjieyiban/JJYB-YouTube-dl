use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use crate::paths::PathsManager;

pub const REQUIRED_TOOLS: [&str; 4] = ["yt-dlp", "ffmpeg", "ffprobe", "deno"];

#[derive(Clone)]
pub struct ToolResolver {
  dirs: Vec<PathBuf>,
}

impl ToolResolver {
  pub fn new(paths: &PathsManager) -> Self {
    Self {
      dirs: paths.tool_dirs(),
    }
  }

  pub fn resolve(&self, tool: &str) -> Option<PathBuf> {
    self.resolve_with(tool, |path, _| probe(path, tool, Duration::from_secs(5)))
  }

  pub fn tool_dirs(&self) -> &[PathBuf] {
    &self.dirs
  }

  pub fn ffmpeg_pair(&self) -> (Option<PathBuf>, Option<PathBuf>) {
    let ffmpeg = self.resolve("ffmpeg");
    if let Some(ref binary) = ffmpeg {
      let sibling = binary.parent().unwrap().join(binary_name("ffprobe"));
      if probe(&sibling, "ffprobe", Duration::from_secs(5)) {
        return (ffmpeg, Some(sibling));
      }
    }
    for dir in &self.dirs {
      let encoder = dir.join(binary_name("ffmpeg"));
      let decoder = dir.join(binary_name("ffprobe"));
      if probe(&encoder, "ffmpeg", Duration::from_secs(5))
        && probe(&decoder, "ffprobe", Duration::from_secs(5))
      {
        return (Some(encoder), Some(decoder));
      }
    }
    (ffmpeg, self.resolve("ffprobe"))
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

fn probe(path: &Path, tool: &str, timeout: Duration) -> bool {
  let Some(arg) = version_arg(tool) else {
    return false;
  };
  if !path.is_absolute() || !path.is_file() {
    return false;
  }
  std::thread::scope(|scope| {
    scope
      .spawn(|| {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
          .enable_all()
          .build()
        else {
          return false;
        };
        runtime.block_on(async {
          use tokio::io::AsyncReadExt;
          const LIMIT: u64 = 64 * 1024;
          let mut command = tokio::process::Command::new(path);
          command
            .arg(arg)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
          #[cfg(windows)]
          command.creation_flags(0x08000000);
          let Ok(mut child) = command.spawn() else {
            return false;
          };
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
          let Ok(Ok((status, out, err))) = output else {
            let _ = child.start_kill();
            return false;
          };
          status.success()
            && out.len() <= LIMIT as usize
            && err.len() <= LIMIT as usize
            && (valid_version(tool, &out) || valid_version(tool, &err))
        })
      })
      .join()
      .unwrap_or(false)
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
  fn skips_invalid_candidates_and_uses_correct_version_flag() {
    let root = std::env::temp_dir();
    let dirs = vec![
      root.join("portable"),
      root.join("managed"),
      root.join("path"),
    ];
    let resolver = ToolResolver { dirs: dirs.clone() };
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
