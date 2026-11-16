use super::UpdateStore;
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Result, UpdaterExt};
use tokio::sync::Mutex;

pub const UPDATE_ENDPOINT: &str =
  "https://github.com/jianjieyiban/JJYB-YouTube-dl/releases/latest/download/latest.json";
pub const RELEASES_URL: &str = "https://github.com/jianjieyiban/JJYB-YouTube-dl/releases";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterStatus {
  pub configured: bool,
  pub reason: Option<String>,
  pub releases_url: &'static str,
}

fn status_for_key(key: &str) -> UpdaterStatus {
  let configured = !key.trim().is_empty();
  UpdaterStatus {
    configured,
    reason: (!configured).then(|| "未配置自有更新公钥，应用自动更新已禁用".into()),
    releases_url: RELEASES_URL,
  }
}

pub fn configuration(app: &AppHandle) -> UpdaterStatus {
  let key = app
    .config()
    .plugins
    .0
    .get("updater")
    .and_then(|value| value.get("pubkey"))
    .and_then(|value| value.as_str())
    .unwrap_or_default();
  status_for_key(key)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
  #[serde(flatten)]
  pub status: UpdaterStatus,
  pub available: bool,
  pub current_version: String,
  pub available_version: Option<String>,
}

#[tauri::command]
pub async fn updater_status(
  app: AppHandle,
  store: State<'_, Mutex<UpdateStore>>,
) -> Result<UpdaterStatus> {
  let status = configuration(&app);
  if !status.configured {
    store.lock().await.clear();
  }
  Ok(status)
}

#[tauri::command]
pub async fn updater_check(
  app: AppHandle,
  store: State<'_, Mutex<UpdateStore>>,
) -> Result<UpdateCheck> {
  let mut state = store.lock().await;
  state.clear();
  let status = configuration(&app);
  if status.configured {
    state.update = app
      .updater_builder()
      .endpoints(vec![UPDATE_ENDPOINT
        .parse()
        .expect("static HTTPS updater URL")])?
      .timeout(std::time::Duration::from_secs(30))
      .build()?
      .check()
      .await?;
  }
  Ok(UpdateCheck {
    status,
    available: state.update.is_some(),
    current_version: app.package_info().version.to_string(),
    available_version: state.update.as_ref().map(|update| update.version.clone()),
  })
}

#[cfg(test)]
mod tests {
  #[test]
  fn unconfigured_check_is_explicit_and_uses_camel_case() {
    let result = super::UpdateCheck {
      status: super::status_for_key(""),
      available: false,
      current_version: "1.0.0".into(),
      available_version: None,
    };
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["configured"], false);
    assert!(json["currentVersion"].is_string());
    assert!(json["reason"].as_str().unwrap().contains("公钥"));
    assert_eq!(json["releasesUrl"], super::RELEASES_URL);
  }

  #[test]
  fn whitespace_key_is_not_configured() {
    assert!(!super::status_for_key(" \n").configured);
  }

  #[test]
  fn clear_invalidates_downloaded_bytes() {
    let mut store = super::UpdateStore {
      bytes: Some(vec![1, 2, 3]),
      ..Default::default()
    };
    store.clear();
    assert!(store.bytes.is_none());
    assert!(store.update.is_none());
  }
}
