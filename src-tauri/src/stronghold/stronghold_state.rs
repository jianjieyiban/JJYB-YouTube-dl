use base64::engine::general_purpose;
use base64::Engine;
use rand::Rng;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, State};
use tauri_plugin_keyring::KeyringExt;
use tauri_plugin_stronghold::stronghold::Stronghold;

pub const CLIENT: &[u8] = b"ovd";
const KR_SERVICE: &str = "com.jelleglebbeek.youtube-dl-gui";
const KR_ACCOUNT: &str = "master_key";

#[derive(Debug, Default, Clone)]
pub struct AuthSecrets {
  pub username: Option<String>,
  pub password: Option<String>,
  pub video_password: Option<String>,
  pub bearer_token: Option<String>,
  pub headers: Vec<String>,
}

pub struct StrongholdState {
  pub snapshot_path: PathBuf,
  pub inner: Mutex<Option<Stronghold>>,
  pub init_error: Mutex<Option<String>>,
}

impl StrongholdState {
  pub const fn new(snapshot_path: PathBuf) -> Self {
    Self {
      snapshot_path,
      inner: Mutex::new(None),
      init_error: Mutex::new(None),
    }
  }
  pub fn load_auth_secrets(&self) -> Result<AuthSecrets, String> {
    let (username, password, video_password, bearer_token, headers) = {
      let guard = self
        .inner
        .lock()
        .map_err(|_| "failed to lock stronghold".to_string())?;
      let sh = guard.as_ref().ok_or_else(|| "vault locked".to_string())?;
      let store = sh.store();

      let get = |key: &str| -> Result<Option<String>, String> {
        match store.get(key.as_bytes()).map_err(|e| e.to_string())? {
          Some(bytes) if !bytes.is_empty() => {
            let s = String::from_utf8(bytes).map_err(|e| e.to_string())?;
            let t = s.trim();
            if t.is_empty() {
              Ok(None)
            } else {
              Ok(Some(t.to_string()))
            }
          }
          _ => Ok(None),
        }
      };

      let username = get("auth.username")?;
      let password = get("auth.password")?;
      let video_password = get("video.password")?;
      let bearer_token = get("auth.bearer")?;

      let mut headers: Vec<String> = Vec::new();
      if let Some(hblob) = get("auth.headers")? {
        for line in hblob.lines() {
          let line = line.trim();
          if line.is_empty() || !line.contains(':') {
            continue;
          }
          headers.push(line.to_string());
        }
      }

      (username, password, video_password, bearer_token, headers)
    };

    Ok(AuthSecrets {
      username,
      password,
      video_password,
      bearer_token,
      headers,
    })
  }
}

fn generate_master_key() -> [u8; 32] {
  let mut key = [0u8; 32];
  rand::rng().fill_bytes(&mut key);
  key
}

fn decode_master_key(b64: &str) -> Result<Vec<u8>, String> {
  let key = general_purpose::STANDARD
    .decode(b64)
    .map_err(|_| "Secure keyring contains a malformed master key".to_string())?;
  if key.len() != 32 {
    return Err("Secure keyring master key must be 32 bytes".to_string());
  }
  Ok(key)
}

fn open_with_key(state: &StrongholdState, key: &[u8]) -> Result<(), String> {
  let snapshot_exists = state.snapshot_path.exists();
  let sh = Stronghold::new(&state.snapshot_path, key.to_vec())
    .map_err(|e| format!("stronghold open failed: {e}"))?;

  if let Err(e) = sh.load_client(CLIENT) {
    if snapshot_exists {
      return Err(format!("load_client failed: {e}"));
    }
    sh.create_client(CLIENT)
      .map_err(|e| format!("create_client failed: {e}"))?;
    sh.write_client(CLIENT)
      .map_err(|e| format!("write_client failed: {e}"))?;
    sh.save().map_err(|e| format!("save failed: {e}"))?;
  }

  *state.inner.lock().unwrap() = Some(sh);
  Ok(())
}

fn create_new_stronghold(state: &StrongholdState, key: &[u8]) -> Result<(), String> {
  open_with_key(state, key)
}

pub fn init(app: &AppHandle, state: &State<StrongholdState>) -> Result<(), String> {
  create_and_store_new(app, state)
}

#[allow(dead_code)]
pub fn init_on_startup(app: &AppHandle, state: &State<StrongholdState>) {
  if !state.snapshot_path.exists() {
    return;
  }

  *state.init_error.lock().unwrap() = None;

  let result = app
    .keyring()
    .get_password(KR_SERVICE, KR_ACCOUNT)
    .map_err(|e| format!("Secure keyring unavailable: {e}"))
    .and_then(|stored| {
      let b64 = stored.ok_or_else(|| "Secure keyring master key is missing".to_string())?;
      let key = decode_master_key(&b64)?;
      open_with_key(state, &key)
    });
  if let Err(e) = result {
    *state.init_error.lock().unwrap() = Some(e);
  }
}

fn create_and_store_new(app: &AppHandle, state: &State<StrongholdState>) -> Result<(), String> {
  let keyring = app.keyring();
  let stored = keyring
    .get_password(KR_SERVICE, KR_ACCOUNT)
    .map_err(|e| format!("Secure keyring unavailable: {e}"))?;
  let key = match stored {
    Some(b64) => decode_master_key(&b64)?,
    None => {
      if state.snapshot_path.exists() {
        return Err("Secure keyring master key is missing for existing vault".to_string());
      }
      let key = generate_master_key();
      let b64 = general_purpose::STANDARD.encode(key);
      keyring
        .set_password(KR_SERVICE, KR_ACCOUNT, &b64)
        .map_err(|e| format!("failed to save key to keyring: {e}"))?;
      key.to_vec()
    }
  };
  create_new_stronghold(state, &key)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn master_key_must_be_valid_base64_and_exactly_32_bytes() {
    assert!(decode_master_key("not base64!").is_err());
    for len in [0, 1, 31, 33, 64] {
      assert!(decode_master_key(&general_purpose::STANDARD.encode(vec![7; len])).is_err());
    }
    let key = vec![7; 32];
    assert_eq!(
      decode_master_key(&general_purpose::STANDARD.encode(&key)).unwrap(),
      key
    );
  }

  #[test]
  fn wrong_key_does_not_modify_existing_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault.hold");
    let state = StrongholdState::new(path.clone());
    open_with_key(&state, &[7; 32]).unwrap();
    let before = std::fs::read(&path).unwrap();
    let locked = StrongholdState::new(path.clone());
    assert!(open_with_key(&locked, &[8; 32]).is_err());
    assert!(locked.inner.lock().unwrap().is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
  }

  #[test]
  fn existing_snapshot_without_client_is_not_recreated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault.hold");
    let sh = Stronghold::new(&path, vec![7; 32]).unwrap();
    sh.save().unwrap();
    drop(sh);
    let before = std::fs::read(&path).unwrap();
    let state = StrongholdState::new(path.clone());
    assert!(open_with_key(&state, &[7; 32]).is_err());
    assert!(state.inner.lock().unwrap().is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
  }
}
