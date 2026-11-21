use std::ffi::OsString;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const PORTABLE_DIR: &str = "jjyb-portable";
const BIN_DIR: &str = "bin";
const SNAP_USER_DATA_ENV: &str = "SNAP_USER_DATA";
const SNAP_USER_COMMON_ENV: &str = "SNAP_USER_COMMON";

#[derive(Clone)]
pub struct PathsManager {
  #[cfg(windows)]
  is_portable_app: bool,
  app_dir: PathBuf,
  bin_dir: PathBuf,
  managed_bin_dir: PathBuf,
  download_dir: PathBuf,
}

impl PathsManager {
  pub fn new(app: &AppHandle) -> std::io::Result<Self> {
    let app_data_dir = app.path().app_data_dir().map_err(std::io::Error::other)?;
    let exe_dir = Self::resolve_executable_path();
    #[cfg(windows)]
    Self::windows_app_dir(exe_dir.clone())?;
    let (app_dir, is_portable_app, is_snap_app) = Self::resolve_app_dir_with(
      &app.config().identifier,
      app_data_dir.clone(),
      exe_dir,
      Self::resolve_snap_user_data_dir(),
    );
    let (bin_dir, is_microsoft_store_app) = Self::resolve_bin_dir(app_dir.clone());

    let mut environment_type = "installed";
    if is_microsoft_store_app {
      environment_type = "microsoft-store"
    }
    if is_portable_app {
      environment_type = "portable";
    }
    if is_snap_app {
      environment_type = "snap";
    }
    tracing::debug!("automatically detected environment: {}", environment_type);
    tracing::debug!(
      "automatically detected paths: app_dir={:?}, bin_dir={:?}",
      app_dir,
      bin_dir
    );
    if is_snap_app {
      if let Some(snap_user_data) = Self::resolve_snap_user_data_dir() {
        tracing::debug!(
          "snap environment detected: {}={:?}",
          SNAP_USER_DATA_ENV,
          snap_user_data
        );
      }
      if let Some(snap_user_common) = Self::resolve_snap_user_common_dir() {
        tracing::debug!(
          "snap environment detected: {}={:?}",
          SNAP_USER_COMMON_ENV,
          snap_user_common
        );
      }
    }

    Ok(Self {
      #[cfg(windows)]
      is_portable_app,
      download_dir: {
        #[cfg(windows)]
        {
          app_dir
            .parent()
            .expect("portable root has a parent")
            .join("Downloads")
        }
        #[cfg(not(windows))]
        {
          app
            .path()
            .download_dir()
            .unwrap_or_else(|_| std::env::current_dir().expect("couldn’t get current dir"))
        }
      },
      app_dir,
      bin_dir,
      managed_bin_dir: app_data_dir.join(BIN_DIR),
    })
  }

  #[cfg(windows)]
  pub fn is_portable_app(&self) -> bool {
    self.is_portable_app
  }

  pub fn app_dir(&self) -> &PathBuf {
    &self.app_dir
  }

  pub fn tmp_dir(&self) -> PathBuf {
    self.app_dir.join("tmp")
  }

  pub fn cache_dir(&self) -> PathBuf {
    self.app_dir.join("cache")
  }

  pub fn download_dir(&self) -> PathBuf {
    self.download_dir.clone()
  }

  pub fn prepare_writable_dirs(&self) -> std::io::Result<()> {
    #[cfg(windows)]
    for dir in [
      self.app_dir.clone(),
      self.bin_dir.clone(),
      self.tmp_dir(),
      self.cache_dir(),
      self.download_dir(),
      self.app_dir.join("webview"),
    ] {
      Self::prepare_directory(&dir)?;
    }
    Ok(())
  }

  #[cfg(any(windows, test))]
  fn prepare_directory(dir: &std::path::Path) -> std::io::Result<()> {
    let prepare = || -> std::io::Result<()> {
      std::fs::create_dir_all(dir)?;
      let probe = tempfile::NamedTempFile::new_in(dir)?;
      probe.as_file().sync_all()?;
      probe.close()?;
      Ok(())
    };
    prepare().map_err(|error| {
      std::io::Error::new(
        error.kind(),
        format!(
          "应用根目录不可写：{}。请移动程序到可写目录；不会回退到 AppData。{error}",
          dir.display()
        ),
      )
    })
  }

  #[cfg(windows)]
  fn windows_app_dir(exe_dir: Option<PathBuf>) -> std::io::Result<PathBuf> {
    exe_dir
      .map(|dir| dir.join(PORTABLE_DIR))
      .ok_or_else(|| std::io::Error::other("无法确定程序所在目录；不会回退到 AppData。"))
  }

  pub fn bin_dir(&self) -> &PathBuf {
    &self.bin_dir
  }

  pub fn tool_dirs(&self) -> Vec<PathBuf> {
    Self::tool_dirs_with(
      Self::resolve_executable_path(),
      self.bin_dir.clone(),
      self.managed_bin_dir.clone(),
      std::env::var_os("PATH"),
    )
  }

  fn tool_dirs_with(
    exe_dir: Option<PathBuf>,
    bin_dir: PathBuf,
    managed_bin_dir: PathBuf,
    path_var: Option<OsString>,
  ) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(exe_dir) = exe_dir {
      let portable = exe_dir.join(PORTABLE_DIR);
      if bin_dir == portable.join(BIN_DIR) {
        dirs.push(portable.join(BIN_DIR));
      }
      dirs.push(exe_dir.join(BIN_DIR));
    }
    dirs.push(bin_dir);
    dirs.push(managed_bin_dir);
    if let Some(path) = path_var {
      dirs.extend(std::env::split_paths(&path).filter(|dir| dir.is_absolute()));
    }
    let mut unique = Vec::new();
    for dir in dirs {
      if !unique.contains(&dir) {
        unique.push(dir);
      }
    }
    unique
  }

  fn resolve_bin_dir(app_dir: PathBuf) -> (PathBuf, bool) {
    Self::resolve_bin_dir_with(
      app_dir,
      Self::resolve_executable_path(),
      Self::resolve_snap_user_common_dir(),
    )
  }

  fn resolve_executable_path() -> Option<PathBuf> {
    if let Ok(exe_path) = std::env::current_exe() {
      if let Some(exe_dir) = exe_path.parent() {
        return Some(exe_dir.to_path_buf());
      }
    }

    None
  }

  fn resolve_snap_user_common_dir() -> Option<PathBuf> {
    std::env::var(SNAP_USER_COMMON_ENV)
      .ok()
      .filter(|value| !value.is_empty())
      .map(PathBuf::from)
  }

  fn resolve_snap_user_data_dir() -> Option<PathBuf> {
    std::env::var(SNAP_USER_DATA_ENV)
      .ok()
      .filter(|value| !value.is_empty())
      .map(PathBuf::from)
  }

  fn resolve_app_dir_with(
    app_id: &str,
    app_data_dir: PathBuf,
    exe_dir: Option<PathBuf>,
    snap_user_data: Option<PathBuf>,
  ) -> (PathBuf, bool, bool) {
    #[cfg(windows)]
    {
      let _ = (app_id, app_data_dir, snap_user_data);
      (
        Self::windows_app_dir(exe_dir).expect("executable directory is required"),
        true,
        false,
      )
    }
    #[cfg(not(windows))]
    {
      if let Some(snap_dir) = snap_user_data {
        return (snap_dir.join(app_id), false, true);
      }
      if let Some(exe_dir) = exe_dir {
        let portable_dir = exe_dir.join(PORTABLE_DIR);
        if portable_dir.exists() {
          return (portable_dir, true, false);
        }
      }

      (app_data_dir, false, false)
    }
  }

  fn resolve_bin_dir_with(
    app_dir: PathBuf,
    exe_dir: Option<PathBuf>,
    snap_user_common: Option<PathBuf>,
  ) -> (PathBuf, bool) {
    if let Some(exe_dir) = exe_dir {
      if app_dir == exe_dir.join(PORTABLE_DIR) {
        return (app_dir.join(BIN_DIR), false);
      }
      let bin_dir = exe_dir.join(BIN_DIR);
      if bin_dir.exists() {
        return (bin_dir, true);
      }
    }

    if let Some(snap_common_dir) = snap_user_common {
      return (snap_common_dir.join(BIN_DIR), false);
    }

    (app_dir.join(BIN_DIR), false)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;

  fn temp_dir(prefix: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("../../../.task-tmp")
      .join(format!("ovd-{}-{}", prefix, uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
  }

  #[cfg(windows)]
  #[test]
  fn markerless_windows_uses_executable_root_and_never_appdata() {
    let base = temp_dir("markerless");
    let exe = base.join("exe");
    let (dir, portable, snap) =
      PathsManager::resolve_app_dir_with("test", base.join("appdata"), Some(exe.clone()), None);
    assert_eq!(dir, exe.join(PORTABLE_DIR));
    assert!(portable);
    assert!(!snap);
    assert!(PathsManager::windows_app_dir(None).is_err());
  }

  #[test]
  fn writable_directory_rejects_file_conflicts() {
    let base = temp_dir("write-conflict");
    let file = base.join("blocked");
    fs::write(&file, b"keep").unwrap();
    assert!(PathsManager::prepare_directory(&file).is_err());
    assert_eq!(fs::read(file).unwrap(), b"keep");
  }

  #[test]
  fn writable_directory_probe_leaves_no_files() {
    let base = temp_dir("write-probe");
    PathsManager::prepare_directory(&base).unwrap();
    assert_eq!(fs::read_dir(base).unwrap().count(), 0);
  }

  #[cfg(not(windows))]
  #[test]
  fn resolve_app_dir_prefers_snap_user_data() {
    let base = temp_dir("snap-app-dir");
    let snap_user_data = base.join("snap-user-data");
    let app_data_dir = base.join("app-data");
    fs::create_dir_all(&snap_user_data).unwrap();
    fs::create_dir_all(&app_data_dir).unwrap();

    let (app_dir, is_portable, is_snap) = PathsManager::resolve_app_dir_with(
      "com.test.app",
      app_data_dir.clone(),
      None,
      Some(snap_user_data.clone()),
    );

    assert_eq!(app_dir, snap_user_data.join("com.test.app"));
    assert!(!is_portable);
    assert!(is_snap);
  }

  #[cfg(not(windows))]
  #[test]
  fn portable_brand_is_isolated_from_upstream() {
    assert_eq!(PORTABLE_DIR, "jjyb-portable");
    let base = temp_dir("upstream-portable");
    let exe_dir = base.join("exe");
    fs::create_dir_all(exe_dir.join("ovd-portable")).unwrap();
    let app_data_dir = base.join("app-data");
    let (resolved, portable, _) = PathsManager::resolve_app_dir_with(
      "com.jianjieyiban.jjybyoutubedl",
      app_data_dir.clone(),
      Some(exe_dir),
      None,
    );
    assert_eq!(resolved, app_data_dir);
    assert!(!portable);
  }

  #[test]
  fn resolve_app_dir_uses_portable_when_present() {
    let base = temp_dir("portable-app-dir");
    let exe_dir = base.join("exe");
    let portable_dir = exe_dir.join(PORTABLE_DIR);
    let app_data_dir = base.join("app-data");
    fs::create_dir_all(&portable_dir).unwrap();
    fs::create_dir_all(&app_data_dir).unwrap();

    let (app_dir, is_portable, is_snap) =
      PathsManager::resolve_app_dir_with("com.test.app", app_data_dir, Some(exe_dir), None);

    assert_eq!(app_dir, portable_dir);
    assert!(is_portable);
    assert!(!is_snap);
  }

  #[cfg(not(windows))]
  #[test]
  fn resolve_app_dir_snap_overrides_portable() {
    let base = temp_dir("snap-over-portable");
    let snap_user_data = base.join("snap-user-data");
    let exe_dir = base.join("exe");
    let portable_dir = exe_dir.join(PORTABLE_DIR);
    let app_data_dir = base.join("app-data");
    fs::create_dir_all(&snap_user_data).unwrap();
    fs::create_dir_all(&portable_dir).unwrap();
    fs::create_dir_all(&app_data_dir).unwrap();

    let (app_dir, is_portable, is_snap) = PathsManager::resolve_app_dir_with(
      "com.test.app",
      app_data_dir,
      Some(exe_dir),
      Some(snap_user_data.clone()),
    );

    assert_eq!(app_dir, snap_user_data.join("com.test.app"));
    assert!(!is_portable);
    assert!(is_snap);
  }

  #[test]
  fn resolve_bin_dir_uses_snap_common_when_set() {
    let base = temp_dir("snap-bin-dir");
    let snap_user_common = base.join("snap-user-common");
    let app_dir = base.join("app");
    fs::create_dir_all(&snap_user_common).unwrap();
    fs::create_dir_all(&app_dir).unwrap();

    let (bin_dir, is_microsoft_store) =
      PathsManager::resolve_bin_dir_with(app_dir, None, Some(snap_user_common.clone()));

    assert_eq!(bin_dir, snap_user_common.join(BIN_DIR));
    assert!(!is_microsoft_store);
  }

  #[test]
  fn resolve_bin_dir_uses_exe_bin_for_microsoft_store() {
    let base = temp_dir("ms-bin-dir");
    let exe_dir = base.join("exe");
    let bin_dir = exe_dir.join(BIN_DIR);
    let app_dir = base.join("app");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&app_dir).unwrap();

    let (resolved, is_microsoft_store) =
      PathsManager::resolve_bin_dir_with(app_dir, Some(exe_dir), None);

    assert_eq!(resolved, bin_dir);
    assert!(is_microsoft_store);
  }

  #[test]
  fn empty_sibling_bin_does_not_shadow_portable_bin() {
    let base = temp_dir("portable-bin-shadow");
    let app_dir = base.join(PORTABLE_DIR);
    fs::create_dir_all(base.join(BIN_DIR)).unwrap();
    fs::create_dir_all(app_dir.join(BIN_DIR)).unwrap();
    let (resolved, store) = PathsManager::resolve_bin_dir_with(app_dir.clone(), Some(base), None);
    assert_eq!(resolved, app_dir.join(BIN_DIR));
    assert!(!store);
  }

  #[test]
  fn resolve_bin_dir_defaults_to_app_dir() {
    let base = temp_dir("default-bin-dir");
    let app_dir = base.join("app");
    fs::create_dir_all(&app_dir).unwrap();

    let (resolved, is_microsoft_store) =
      PathsManager::resolve_bin_dir_with(app_dir.clone(), None, None);

    assert_eq!(resolved, app_dir.join(BIN_DIR));
    assert!(!is_microsoft_store);
  }

  #[test]
  fn tool_dirs_include_managed_fallback_in_portable_mode() {
    let base = temp_dir("tool-dirs-portable-managed");
    let exe_dir = base.join("exe");
    let portable_bin = exe_dir.join(PORTABLE_DIR).join(BIN_DIR);
    let managed_bin = base.join("managed").join(BIN_DIR);
    fs::create_dir_all(&portable_bin).unwrap();

    let dirs = PathsManager::tool_dirs_with(
      Some(exe_dir.clone()),
      portable_bin.clone(),
      managed_bin,
      None,
    );

    assert_eq!(dirs[0], portable_bin);
    assert_eq!(dirs[1], exe_dir.join(BIN_DIR));
    assert!(dirs.contains(&base.join("managed").join(BIN_DIR)));
  }

  #[test]
  fn tool_dirs_deduplicate_managed_and_bin_dir() {
    let base = temp_dir("tool-dirs-dedup");
    let bin_dir = base.join("app").join(BIN_DIR);
    let path_var =
      std::env::join_paths([base.join("path-a"), base.join("path-a").join("sub")]).ok();

    let dirs = PathsManager::tool_dirs_with(None, bin_dir.clone(), bin_dir.clone(), path_var);

    assert_eq!(
      dirs,
      vec![
        bin_dir,
        base.join("path-a"),
        base.join("path-a").join("sub")
      ]
    );
  }
}
