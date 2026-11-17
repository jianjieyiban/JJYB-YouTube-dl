use crate::portable_update::{verify_executable, PortableManifest};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

#[derive(serde::Serialize, Clone)]
pub struct UpdateProgress {
  pub received: usize,
  pub total: Option<u64>,
}

#[derive(Default)]
pub struct UpdateStore {
  pub update: Option<PortableManifest>,
  pub raw_manifest: Option<Vec<u8>>,
  pub signature: Option<String>,
  pub bytes: Option<Vec<u8>>,
}

impl UpdateStore {
  pub fn clear(&mut self) {
    self.update = None;
    self.raw_manifest = None;
    self.signature = None;
    self.bytes = None;
  }
}

#[tauri::command]
pub async fn updater_download(
  app: AppHandle,
  store: State<'_, Mutex<UpdateStore>>,
) -> Result<(), String> {
  let mut state = store
    .try_lock()
    .map_err(|_| "更新操作正在进行，请勿重复操作")?;
  state.bytes = None;
  if !super::configuration(&app).configured {
    state.clear();
    return Err("当前版本不支持便携一键更新".into());
  }
  let manifest = state.update.clone().ok_or("请先检查可用更新")?;
  let result: Result<Vec<u8>, String> = async {
    let client = super::update_client()?;
    let mut response = client
      .get(&manifest.url)
      .send()
      .await
      .map_err(|e| e.to_string())?
      .error_for_status()
      .map_err(|e| e.to_string())?;
    if response
      .content_length()
      .is_some_and(|size| size != manifest.size)
    {
      return Err("更新文件长度与签名清单不符".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
      if bytes.len() as u64 + chunk.len() as u64 > manifest.size {
        return Err("更新文件超过签名大小".into());
      }
      bytes.extend_from_slice(&chunk);
      let _ = app.emit(
        "updater_download_progress",
        UpdateProgress {
          received: bytes.len(),
          total: Some(manifest.size),
        },
      );
    }
    verify_executable(&bytes, &manifest)?;
    Ok(bytes)
  }
  .await;
  match result {
    Ok(bytes) => {
      state.bytes = Some(bytes);
      Ok(())
    }
    Err(error) => {
      state.clear();
      let _ = app.emit("updater_error", &error);
      Err(error)
    }
  }
}
