//! Same-executable portable update transactions. No downloaded helper or shell is executed.
//! The local account and installation directory ACL are the trust boundary; this is not
//! a defense against another process already able to replace this application's EXE.
pub const UPDATE_PUBLIC_KEY: &str =
  "569d7c1c34b0d8fbf80a59dc876776296f73ab0dd956df5a42f8e7104edaaf2e";

#[cfg(windows)]
pub use windows::{confirm_ready, launch, stage, startup, StagedTransaction};

#[cfg(not(windows))]
pub fn startup() -> Result<bool, String> {
  Ok(false)
}

#[cfg(not(windows))]
pub fn confirm_ready() -> Result<(), String> {
  Ok(())
}

#[cfg(windows)]
mod windows {
  use crate::portable_update::{verify_executable, verify_manifest, PortableManifest};
  use serde::{Deserialize, Serialize};
  use sha2::{Digest, Sha256};
  use std::fs::{self, File, OpenOptions};
  use std::io::{Read, Write};
  use std::os::windows::ffi::OsStrExt;
  use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
  use std::os::windows::io::AsRawHandle;
  use std::path::{Component, Path, PathBuf, Prefix};
  use std::process::{Child, Command};
  use std::sync::Mutex;
  use std::time::{Duration, Instant};
  use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
  use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, MoveFileExW, BY_HANDLE_FILE_INFORMATION,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
  };
  use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
  };

  const HELPER: &str = "--jjyb-portable-helper";
  const CHILD: &str = "--jjyb-portable-child";
  const PREFIX: &str = ".jjyb-update-";
  const READY_TIMEOUT: Duration = Duration::from_secs(90);
  static PENDING: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);

  fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
  }
  #[cfg(test)]
  pub(super) fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
  }
  fn hash(path: &Path) -> Result<String, String> {
    let mut file = plain_file(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
      let n = file.read(&mut buf).map_err(err)?;
      if n == 0 {
        break;
      }
      hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
  }
  fn token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
  }
  fn is_token(s: &str) -> bool {
    s.len() == 32
      && s
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
  }
  pub(super) fn transaction_name(s: &str) -> bool {
    s.strip_prefix(PREFIX).is_some_and(is_token)
  }

  pub(super) fn validate_target_name(s: &str) -> Result<(), String> {
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if s.is_empty()
      || !s.to_ascii_lowercase().ends_with(".exe")
      || s
        .chars()
        .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
      || s.ends_with(['.', ' '])
      || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
      || (stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.as_bytes()[3].is_ascii_digit())
    {
      return Err("Unsafe executable name".into());
    }
    Ok(())
  }
  pub(super) fn validate_absolute(path: &Path) -> Result<(), String> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
      || !matches!(components.next(), Some(Component::RootDir))
      || components.any(|c| !matches!(c, Component::Normal(_)))
    {
      return Err("Only local absolute non-traversing paths are allowed".into());
    }
    // components() normalizes '.', so reject it in the original spelling as well.
    if path
      .as_os_str()
      .to_string_lossy()
      .split(['\\', '/'])
      .any(|s| s == "." || s == "..")
    {
      return Err("Path traversal is not allowed".into());
    }
    Ok(())
  }
  struct Handle(HANDLE);
  impl Drop for Handle {
    fn drop(&mut self) {
      unsafe {
        CloseHandle(self.0);
      }
    }
  }

  // Keep every ancestor open without FILE_SHARE_DELETE throughout mutation. This
  // prevents junction/rename substitution between validation and file operations.
  pub(super) fn guard_path(path: &Path) -> Result<(Vec<File>, PathBuf), String> {
    validate_absolute(path)?;
    let mut guards = Vec::new();
    let parent = path.parent().ok_or("Missing parent directory")?;
    let mut chain: Vec<_> = parent.ancestors().collect();
    chain.reverse();
    for ancestor in chain {
      let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(ancestor)
        .map_err(err)?;
      if file.metadata().map_err(err)?.file_attributes() & 0x400 != 0 {
        return Err("Reparse point in update path".into());
      }
      guards.push(file);
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
      if metadata.file_attributes() & 0x400 != 0 {
        return Err("Reparse update file".into());
      }
    }
    Ok((guards, path.to_path_buf()))
  }
  pub(super) fn plain_file(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
      .read(true)
      .share_mode(FILE_SHARE_READ)
      .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
      .open(path)
      .map_err(err)?;
    let meta = file.metadata().map_err(err)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
      return Err(err(std::io::Error::last_os_error()));
    }
    if !meta.is_file() || meta.file_attributes() & 0x400 != 0 || info.nNumberOfLinks != 1 {
      return Err("Update input must be a regular non-reparse single-link file".into());
    }
    Ok(file)
  }
  fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(path)
      .map_err(err)?;
    f.write_all(bytes).map_err(err)?;
    f.sync_all().map_err(err)
  }
  fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    let from: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
      MoveFileExW(
        from.as_ptr(),
        to.as_ptr(),
        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
      )
    } == 0
    {
      return Err(err(std::io::Error::last_os_error()));
    }
    Ok(())
  }
  fn lock(parent: &Path) -> Result<File, String> {
    OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .share_mode(0)
      .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
      .open(parent.join(".jjyb-portable-update.lock"))
      .map_err(err)
      .and_then(|f| {
        if f.metadata().map_err(err)?.file_attributes() & 0x400 != 0 {
          return Err("Reparse update lock".into());
        }
        Ok(f)
      })
  }
  #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
  enum Phase {
    Staged,
    Replacing,
    AwaitingReady,
    Committed,
    RollingBack,
    RolledBack,
  }
  #[derive(Serialize, Deserialize)]
  #[serde(deny_unknown_fields)]
  struct Journal {
    schema: u32,
    target: String,
    old_hash: String,
    old_version: String,
    nonce: String,
    parent_pid: u32,
    parent_birth: u64,
    phase: Phase,
  }
  fn save(dir: &Path, journal: &Journal) -> Result<(), String> {
    let pending = dir.join(format!("journal-{}.tmp", token()));
    write_new(&pending, &serde_json::to_vec(journal).map_err(err)?)?;
    move_file(&pending, &dir.join("journal.json"))
  }
  fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut f = plain_file(path)?;
    if f.metadata().map_err(err)?.len() > limit {
      return Err("Transaction input too large".into());
    }
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(err)?;
    Ok(bytes)
  }
  fn load(dir: &Path) -> Result<(Journal, PortableManifest), String> {
    if !dir
      .file_name()
      .and_then(|s| s.to_str())
      .is_some_and(transaction_name)
    {
      return Err("Invalid transaction directory".into());
    }
    let journal: Journal =
      serde_json::from_slice(&read_limited(&dir.join("journal.json"), 8192)?).map_err(err)?;
    validate_target_name(&journal.target)?;
    if journal.schema != 1
      || !is_token(&journal.nonce)
      || journal.old_hash.len() != 64
      || !journal.old_hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
      return Err("Invalid transaction journal".into());
    }
    let manifest = verify_manifest(
      &read_limited(&dir.join("manifest.json"), 1024 * 1024)?,
      &String::from_utf8(read_limited(&dir.join("manifest.sig"), 1024)?).map_err(err)?,
      super::UPDATE_PUBLIC_KEY,
      &journal.old_version,
    )?;
    Ok((journal, manifest))
  }
  fn birth(handle: HANDLE) -> Result<u64, String> {
    use windows_sys::Win32::Foundation::FILETIME;
    let mut created = FILETIME {
      dwLowDateTime: 0,
      dwHighDateTime: 0,
    };
    let mut exited = created;
    let mut kernel = created;
    let mut user = created;
    if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
      return Err(err(std::io::Error::last_os_error()));
    }
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
  }
  fn process(pid: u32, expected_birth: u64, expected_image: &Path) -> Result<Handle, String> {
    let handle = Handle(unsafe {
      OpenProcess(
        PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
        0,
        pid,
      )
    });
    if handle.0.is_null() {
      return Err(err(std::io::Error::last_os_error()));
    }
    if birth(handle.0)? != expected_birth {
      return Err("Process identity changed".into());
    }
    let mut image = vec![0u16; 32768];
    let mut size = image.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle.0, 0, image.as_mut_ptr(), &mut size) } == 0 {
      return Err(err(std::io::Error::last_os_error()));
    }
    let actual = PathBuf::from(String::from_utf16(&image[..size as usize]).map_err(err)?);
    if fs::canonicalize(actual).map_err(err)? != fs::canonicalize(expected_image).map_err(err)? {
      return Err("Unexpected parent process image".into());
    }
    Ok(handle)
  }
  fn current_birth() -> Result<u64, String> {
    birth(unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() })
  }

  pub struct StagedTransaction {
    dir: PathBuf,
    target: PathBuf,
    nonce: String,
    guards: Vec<File>,
    lock: File,
  }

  /// Verifies against the compiled key, not any disk-supplied key. All paths are
  /// derived from current_exe; callers cannot supply destination paths.
  pub fn stage(
    manifest: &PortableManifest,
    raw_manifest: &[u8],
    signature: &str,
    bytes: &[u8],
  ) -> Result<StagedTransaction, String> {
    let verified = verify_manifest(
      raw_manifest,
      signature,
      super::UPDATE_PUBLIC_KEY,
      env!("CARGO_PKG_VERSION"),
    )?;
    if serde_json::to_vec(manifest).map_err(err)? != serde_json::to_vec(&verified).map_err(err)? {
      return Err("Manifest differs from signed bytes".into());
    }
    verify_executable(bytes, &verified)?;
    let target = std::env::current_exe().map_err(err)?;
    let (mut guards, _) = guard_path(&target)?;
    let parent = target.parent().ok_or("Missing executable directory")?;
    let lock = lock(parent)?;
    reject_pending(parent)?;
    let target_name = target
      .file_name()
      .and_then(|s| s.to_str())
      .ok_or("Invalid executable name")?;
    validate_target_name(target_name)?;
    let source = plain_file(&target)?;
    if source.metadata().map_err(err)?.permissions().readonly() {
      return Err("Portable executable is read-only; update was not staged".into());
    }
    let dir = parent.join(format!("{PREFIX}{}", token()));
    fs::create_dir(&dir).map_err(err)?;
    guards.extend(guard_path(&dir.join("journal.json"))?.0);
    let nonce = token();
    // Failed staging leaves only an adjacent inert directory; never delete a tree
    // supplied by the journal or follow links during cleanup.
    write_new(&dir.join("manifest.json"), raw_manifest)?;
    write_new(&dir.join("manifest.sig"), signature.as_bytes())?;
    write_new(&dir.join("new.exe"), bytes)?;
    let mut helper = OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(dir.join("helper.exe"))
      .map_err(err)?;
    std::io::copy(&mut &source, &mut helper).map_err(err)?;
    helper.sync_all().map_err(err)?;
    drop(helper);
    let mut backup = OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(dir.join("old.exe"))
      .map_err(err)?;
    let mut original = plain_file(&target)?;
    std::io::copy(&mut original, &mut backup).map_err(err)?;
    backup.sync_all().map_err(err)?;
    drop(backup);
    let old_hash = hash(&target)?;
    if hash(&dir.join("old.exe"))? != old_hash {
      return Err("Backup copy differs from running executable".into());
    }
    if hash(&dir.join("helper.exe"))? != old_hash {
      return Err("Helper copy differs from running executable".into());
    }
    save(
      &dir,
      &Journal {
        schema: 1,
        target: target_name.into(),
        old_hash,
        old_version: env!("CARGO_PKG_VERSION").into(),
        nonce: nonce.clone(),
        parent_pid: std::process::id(),
        parent_birth: current_birth()?,
        phase: Phase::Staged,
      },
    )?;
    Ok(StagedTransaction {
      dir,
      target,
      nonce,
      guards,
      lock,
    })
  }

  /// Returns only after helper has opened and authenticated this process. Caller
  /// must save settings first and exit normally immediately after Ok(()).
  pub fn launch(staged: StagedTransaction) -> Result<(), String> {
    let StagedTransaction {
      dir,
      target,
      nonce,
      guards,
      lock,
    } = staged;
    let (_dir_guards, _) = guard_path(&dir.join("helper.exe"))?;
    let helper_file = plain_file(&dir.join("helper.exe"))?;
    if hash(&dir.join("helper.exe"))? != hash(&target)? {
      return Err("Untrusted helper copy".into());
    }
    drop(lock);
    let mut helper = Command::new(dir.join("helper.exe"))
      .arg(HELPER)
      .arg("install")
      .arg(&nonce)
      .spawn()
      .map_err(err)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
      if marker(&dir, &format!("helper-ready-{nonce}"), &nonce) {
        break;
      }
      if helper.try_wait().map_err(err)?.is_some() || Instant::now() >= deadline {
        let _ = helper.kill();
        let _ = helper.wait();
        return Err(
          "Update helper failed before parent exit; current application remains running".into(),
        );
      }
      std::thread::sleep(Duration::from_millis(25));
    }
    drop(helper_file);
    drop(guards);
    Ok(())
  }
  fn marker(dir: &Path, name: &str, nonce: &str) -> bool {
    read_limited(&dir.join(name), 64).is_ok_and(|bytes| bytes == nonce.as_bytes())
  }
  pub(super) fn restore_backup(dir: &Path, target: &Path, expected: &str) -> Result<(), String> {
    let backup = dir.join("old.exe");
    if hash(target).is_ok_and(|actual| actual == expected) {
      return Ok(());
    }
    if !backup.exists() {
      return if hash(target)? == expected {
        Ok(())
      } else {
        Err("Missing valid rollback backup".into())
      };
    }
    let file = plain_file(&backup)?;
    if hash(&backup)? != expected {
      return Err("Rollback backup hash mismatch".into());
    }
    drop(file);
    move_file(&backup, target)
  }
  #[derive(Debug, PartialEq)]
  pub(super) enum Decision {
    Wait,
    Commit,
    Rollback,
  }
  pub(super) fn readiness(ready: bool, exited: bool, timed_out: bool) -> Decision {
    if exited || timed_out {
      Decision::Rollback
    } else if ready {
      Decision::Commit
    } else {
      Decision::Wait
    }
  }
  fn wait_ready(child: &mut Child, dir: &Path, nonce: &str) -> Result<(), String> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
      match readiness(
        marker(dir, "app-ready", nonce),
        child.try_wait().map_err(err)?.is_some(),
        Instant::now() >= deadline,
      ) {
        Decision::Commit => return Ok(()),
        Decision::Rollback => {
          // Wait for actual termination before attempting rollback of the mapped EXE.
          if child.try_wait().map_err(err)?.is_none() {
            child.kill().map_err(err)?;
          }
          child.wait().map_err(err)?;
          return Err("Updated application exited or did not complete app_ready in time".into());
        }
        Decision::Wait => std::thread::sleep(Duration::from_millis(50)),
      }
    }
  }
  fn rollback(dir: &Path, target: &Path, journal: &mut Journal) -> Result<(), String> {
    journal.phase = Phase::RollingBack;
    save(dir, journal)?;
    restore_backup(dir, target, &journal.old_hash)?;
    journal.phase = Phase::RolledBack;
    save(dir, journal)?;
    Ok(())
  }

  fn helper(mode: &str, nonce: &str) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(err)?;
    let dir = executable.parent().ok_or("Missing helper directory")?;
    let (_guards, _) = guard_path(&dir.join("journal.json"))?;
    let parent = dir.parent().ok_or("Missing installation directory")?;
    let _lock = lock(parent)?;
    let (mut journal, manifest) = load(dir)?;
    if nonce != journal.nonce {
      return Err("Invalid helper capability".into());
    }
    let target = parent.join(&journal.target);
    let (_target_guards, _) = guard_path(&target)?;
    if mode == "install" && hash(&dir.join("old.exe"))? != journal.old_hash {
      return Err("Backup verification failed before parent exit".into());
    }
    let parent_process = process(journal.parent_pid, journal.parent_birth, &target)?;
    if mode == "install" {
      if journal.phase != Phase::Staged
        || hash(&executable)? != journal.old_hash
        || hash(&target)? != journal.old_hash
      {
        return Err("Helper is not the exact running application copy".into());
      }
      let payload = read_limited(&dir.join("new.exe"), 512 * 1024 * 1024)?;
      verify_executable(&payload, &manifest)?;
    } else if mode != "recover" || hash(&executable)? != hash(&target)? {
      return Err("Untrusted recovery helper".into());
    }
    let ready_path = dir.join(format!("helper-ready-{nonce}"));
    write_new(&ready_path, nonce.as_bytes())?;
    if unsafe { WaitForSingleObject(parent_process.0, 30_000) } != WAIT_OBJECT_0 {
      return Err("Parent did not exit; update was not applied".into());
    }
    if mode == "recover" {
      rollback(dir, &target, &mut journal)?;
      drop(_lock);
      Command::new(&target).spawn().map_err(err)?;
      return Ok(());
    }
    journal.phase = Phase::Replacing;
    save(dir, &journal)?;
    // The backup was flushed during stage, before asking the UI to exit.
    // Replace atomically so the original launch path remains bootable.
    if hash(&dir.join("old.exe"))? != journal.old_hash {
      return Err("Backup verification failed".into());
    }
    let install = (|| {
      let staged = plain_file(&dir.join("new.exe"))?;
      verify_executable(
        &read_limited(&dir.join("new.exe"), 512 * 1024 * 1024)?,
        &manifest,
      )?;
      drop(staged);
      move_file(&dir.join("new.exe"), &target)?;
      journal.phase = Phase::AwaitingReady;
      save(dir, &journal)?;
      let mut child = Command::new(&target)
        .arg(CHILD)
        .arg(dir.file_name().unwrap())
        .arg(nonce)
        .spawn()
        .map_err(err)?;
      if let Err(error) = wait_ready(&mut child, dir, nonce) {
        if child.try_wait().map_err(err)?.is_none() {
          child.kill().map_err(err)?;
          child.wait().map_err(err)?;
        }
        return Err(error);
      }
      Ok::<(), String>(())
    })();
    if let Err(error) = install {
      rollback(dir, &target, &mut journal)?;
      drop(_lock);
      Command::new(&target).spawn().map_err(err)?;
      return Err(error);
    }
    journal.phase = Phase::Committed;
    save(dir, &journal)
  }

  fn reject_pending(parent: &Path) -> Result<(), String> {
    for entry in fs::read_dir(parent).map_err(err)? {
      let entry = entry.map_err(err)?;
      if !entry.file_name().to_str().is_some_and(transaction_name) {
        continue;
      }
      let (_guards, _) = guard_path(&entry.path().join("journal.json"))?;
      if !entry.path().join("journal.json").exists() {
        continue;
      }
      let (journal, _) = load(&entry.path())?;
      if !matches!(journal.phase, Phase::Committed | Phase::RolledBack) {
        return Err("An unfinished portable update requires restart recovery".into());
      }
    }
    Ok(())
  }

  /// Called before Tauri/single-instance setup. true means helper/recovery ran;
  /// the caller must not start the regular UI (including when recovery restarts).
  pub fn startup() -> Result<bool, String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|s| s == HELPER) {
      if args.len() != 3 {
        return Err("Invalid helper arguments".into());
      }
      helper(
        args[1].to_str().ok_or("Invalid helper mode")?,
        args[2].to_str().ok_or("Invalid helper token")?,
      )?;
      return Ok(true);
    }
    let target = std::env::current_exe().map_err(err)?;
    let (_guards, _) = guard_path(&target)?;
    let parent = target.parent().ok_or("Missing executable directory")?;
    if args.first().is_some_and(|s| s == CHILD) {
      if args.len() != 3 {
        return Err("Invalid update child arguments".into());
      }
      let name = args[1].to_str().ok_or("Invalid transaction name")?;
      if !transaction_name(name) {
        return Err("Invalid child transaction directory".into());
      }
      let dir = parent.join(name);
      let (_dir_guards, _) = guard_path(&dir.join("journal.json"))?;
      let (journal, manifest) = load(&dir)?;
      if journal.phase != Phase::AwaitingReady
        || args[2] != journal.nonce.as_str()
        || target.file_name() != Some(std::ffi::OsStr::new(&journal.target))
      {
        return Err("Invalid update child handshake".into());
      }
      verify_executable(&read_limited(&target, 512 * 1024 * 1024)?, &manifest)?;
      *PENDING.lock().map_err(err)? = Some((dir, journal.nonce));
      return Ok(false);
    }
    // Installed/read-only copies with no transaction do not need write access.
    let has_transaction = fs::read_dir(parent)
      .map_err(err)?
      .any(|entry| entry.is_ok_and(|e| e.file_name().to_str().is_some_and(transaction_name)));
    if !has_transaction {
      return Ok(false);
    }
    let lock = match lock(parent) {
      Ok(lock) => lock,
      // An active helper owns recovery. Do not let a manually launched instance
      // race it or mask a readiness failure via the single-instance plugin.
      Err(error) => return Err(format!("Portable update is active or locked: {error}")),
    };
    for entry in fs::read_dir(parent).map_err(err)? {
      let entry = entry.map_err(err)?;
      if !entry.file_name().to_str().is_some_and(transaction_name) {
        continue;
      }
      let dir = entry.path();
      let (_dir_guards, _) = guard_path(&dir.join("journal.json"))?;
      if !dir.join("journal.json").exists() {
        continue;
      }
      let (mut journal, _) = load(&dir)?;
      if target.file_name() != Some(std::ffi::OsStr::new(&journal.target)) {
        return Err("Adjacent transaction belongs to a different executable".into());
      }
      if journal.phase == Phase::Staged {
        journal.phase = Phase::RolledBack;
        save(&dir, &journal)?;
        continue;
      }
      if matches!(journal.phase, Phase::Committed | Phase::RolledBack) {
        continue;
      }
      // Never execute a recovery image merely because its hash is in a journal.
      // Copy this trusted currently-running EXE, then launch it under a new nonce.
      journal.nonce = token();
      journal.parent_pid = std::process::id();
      journal.parent_birth = current_birth()?;
      save(&dir, &journal)?;
      let recovery = dir.join(format!("recovery-{}.exe", token()));
      let mut copy = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&recovery)
        .map_err(err)?;
      let mut source = plain_file(&target)?;
      std::io::copy(&mut source, &mut copy).map_err(err)?;
      copy.sync_all().map_err(err)?;
      drop(source);
      drop(copy);
      let pin = plain_file(&recovery)?;
      if hash(&recovery)? != hash(&target)? {
        return Err("Recovery copy mismatch".into());
      }
      // New capability also invalidates stale readiness markers.
      drop(lock);
      let mut helper = Command::new(&recovery)
        .arg(HELPER)
        .arg("recover")
        .arg(&journal.nonce)
        .spawn()
        .map_err(err)?;
      let deadline = Instant::now() + Duration::from_secs(15);
      while !marker(
        &dir,
        &format!("helper-ready-{}", journal.nonce),
        &journal.nonce,
      ) {
        if helper.try_wait().map_err(err)?.is_some() || Instant::now() >= deadline {
          let _ = helper.kill();
          let _ = helper.wait();
          return Err("Recovery helper failed before parent exit".into());
        }
        std::thread::sleep(Duration::from_millis(25));
      }
      drop(pin);
      return Ok(true);
    }
    Ok(false)
  }

  /// Invoke only at the very end of the real app_ready command. Startup/setup
  /// is deliberately insufficient to acknowledge a successful update.
  pub fn confirm_ready() -> Result<(), String> {
    let mut pending = PENDING.lock().map_err(err)?;
    if let Some((dir, nonce)) = pending.as_ref() {
      let (_guards, _) = guard_path(&dir.join("app-ready"))?;
      let (journal, _) = load(dir)?;
      if journal.phase != Phase::AwaitingReady || journal.nonce != *nonce {
        return Err("Stale application readiness capability".into());
      }
      write_new(&dir.join("app-ready"), nonce.as_bytes())?;
      *pending = None;
    }
    Ok(())
  }
}

#[cfg(all(test, windows))]
mod tests {
  use super::windows::*;
  use std::fs;

  #[test]
  fn accepts_chinese_and_space_paths_without_touching_configuration() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("中文 便携目录");
    fs::create_dir(&parent).unwrap();
    let target = parent.join("JJYB 下载器.exe");
    fs::write(&target, b"old").unwrap();
    fs::create_dir(parent.join("bin")).unwrap();
    fs::write(parent.join("bin/tool.exe"), b"tool").unwrap();
    fs::write(parent.join("config.json"), b"settings").unwrap();
    let (_guards, _) = guard_path(&target).unwrap();
    let dir = parent.join(format!(".jjyb-update-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("old.exe"), b"old").unwrap();
    fs::write(&target, b"new").unwrap();
    restore_backup(&dir, &target, &digest(b"old")).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(fs::read(parent.join("config.json")).unwrap(), b"settings");
    assert_eq!(fs::read(parent.join("bin/tool.exe")).unwrap(), b"tool");
  }

  #[test]
  fn rejects_escape_unc_and_malformed_transaction_names() {
    for name in [
      "../evil.exe",
      "C:\\evil.exe",
      "a/b.exe",
      "a:b.exe",
      "CON.exe",
      "a.exe.",
    ] {
      assert!(validate_target_name(name).is_err(), "{name}");
    }
    for path in [
      r"\\server\share\a.exe",
      r"\\?\UNC\server\share\a.exe",
      r"C:\safe\..\a.exe",
    ] {
      assert!(
        validate_absolute(std::path::Path::new(path)).is_err(),
        "{path}"
      );
    }
    for name in [
      ".jjyb-update-../x",
      ".jjyb-update-0000",
      ".jjyb-update-XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
    ] {
      assert!(!transaction_name(name));
    }
  }

  #[test]
  fn rollback_is_idempotent_after_interrupted_restore() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("app.exe");
    fs::write(&target, b"new").unwrap();
    fs::write(root.path().join("old.exe"), b"old").unwrap();
    restore_backup(root.path(), &target, &digest(b"old")).unwrap();
    restore_backup(root.path(), &target, &digest(b"old")).unwrap();
    assert_eq!(fs::read(target).unwrap(), b"old");
  }

  #[test]
  fn corrupt_backup_never_replaces_target() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("app.exe");
    fs::write(&target, b"new").unwrap();
    fs::write(root.path().join("old.exe"), b"corrupt").unwrap();
    assert!(restore_backup(root.path(), &target, &digest(b"old")).is_err());
    assert_eq!(fs::read(target).unwrap(), b"new");
  }

  #[test]
  fn timeout_and_crash_require_rollback_but_ready_commits() {
    assert_eq!(readiness(false, false, true), Decision::Rollback);
    assert_eq!(readiness(false, true, false), Decision::Rollback);
    assert_eq!(readiness(false, false, false), Decision::Wait);
    assert_eq!(readiness(true, false, false), Decision::Commit);
  }

  #[test]
  fn locked_destination_preserves_backup_on_replace_failure() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("app.exe");
    fs::write(&target, b"new").unwrap();
    fs::write(root.path().join("old.exe"), b"old").unwrap();
    let pinned = plain_file(&target).unwrap();
    assert!(restore_backup(root.path(), &target, &digest(b"old")).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(fs::read(root.path().join("old.exe")).unwrap(), b"old");
    drop(pinned);
    restore_backup(root.path(), &target, &digest(b"old")).unwrap();
    assert_eq!(fs::read(target).unwrap(), b"old");
  }

  #[test]
  fn rejects_hardlinked_transaction_input() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("source.exe");
    fs::write(&file, b"content").unwrap();
    fs::hard_link(&file, root.path().join("linked.exe")).unwrap();
    assert!(plain_file(&file).is_err());
  }

  #[test]
  fn rejects_directory_junction_ancestor() {
    use std::process::Command;
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    let link = root.path().join("junction");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("app.exe"), b"old").unwrap();
    let status = Command::new("cmd")
      .args(["/C", "mklink", "/J"])
      .arg(&link)
      .arg(&real)
      .status()
      .unwrap();
    assert!(status.success(), "junction fixture creation failed");
    assert!(guard_path(&link.join("app.exe")).is_err());
    fs::remove_dir(link).unwrap();
  }
}
