use super::UpdateStore;
use crate::portable_update::{verify_manifest, PortableManifest};
use crate::state::config_models::NetworkSettings;
use tauri::Manager;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;

pub const OWNER: &str = "jianjieyiban";
pub const REPO: &str = "JJYB-YouTube-dl";
pub const API_LATEST: &str =
  "https://api.github.com/repos/jianjieyiban/JJYB-YouTube-dl/releases/latest";
pub const RELEASES_URL: &str = "https://github.com/jianjieyiban/JJYB-YouTube-dl/releases";
pub use crate::portable_transaction::UPDATE_PUBLIC_KEY;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterStatus {
  pub configured: bool,
  pub reason: Option<String>,
  pub releases_url: &'static str,
}

pub fn configuration(app: &AppHandle) -> UpdaterStatus {
  #[cfg(all(windows, target_arch = "x86_64"))]
  let supported = app.state::<crate::paths::PathsManager>().is_portable_app();
  #[cfg(not(all(windows, target_arch = "x86_64")))]
  let supported = {
    let _ = app;
    false
  };
  UpdaterStatus {
    configured: supported,
    reason: (!supported).then(|| "一键更新仅支持 Windows x64 便携版，请从自有仓库手动下载".into()),
    releases_url: RELEASES_URL,
  }
}

fn trusted_update_redirect(url: &url::Url) -> bool {
  url.scheme() == "https"
    && url.username().is_empty()
    && url.password().is_none()
    && url.port_or_known_default() == Some(443)
    && matches!(
      url.host_str(),
      Some("github.com" | "release-assets.githubusercontent.com" | "objects.githubusercontent.com")
    )
}

fn trusted_api_request(url: &url::Url) -> bool {
  url.scheme() == "https"
    && url.username().is_empty()
    && url.password().is_none()
    && url.port_or_known_default() == Some(443)
    && url.host_str() == Some("api.github.com")
    && url.path().starts_with(&format!("/repos/{OWNER}/{REPO}/"))
}

pub fn update_client(network: &NetworkSettings) -> Result<reqwest::Client, String> {
  let mut builder = reqwest::Client::builder();
  if network.enable_proxy == Some(true) {
    let proxy = network
      .proxy
      .as_deref()
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .ok_or("已启用代理，但代理地址为空")?;
    builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| format!("无效更新代理: {e}"))?);
  }
  builder
    .user_agent("JJYB-YouTube-dl-Updater")
    .connect_timeout(std::time::Duration::from_secs(15))
    // 持续有数据的慢速下载不设总时限；长时间无数据仍应失败。
    .read_timeout(std::time::Duration::from_secs(60))
    .redirect(reqwest::redirect::Policy::custom(|attempt| {
      let url = attempt.url();
      if attempt.previous().len() >= 5 {
        return attempt.error("Too many update redirects");
      }
      if trusted_update_redirect(url) || trusted_api_request(url) {
        attempt.follow()
      } else {
        attempt.error("Untrusted update redirect")
      }
    }))
    .build()
    .map_err(|e| e.to_string())
}

pub async fn bounded_download(
  client: &reqwest::Client,
  url: &str,
  limit: u64,
) -> Result<Vec<u8>, String> {
  let mut response = client
    .get(url)
    .timeout(std::time::Duration::from_secs(60))
    .send()
    .await
    .map_err(|e| e.to_string())?
    .error_for_status()
    .map_err(|e| e.to_string())?;
  if response.content_length().is_some_and(|len| len > limit) {
    return Err("更新响应超过大小限制".into());
  }
  let mut bytes = Vec::new();
  while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
    if bytes.len() as u64 + chunk.len() as u64 > limit {
      return Err("更新响应超过大小限制".into());
    }
    bytes.extend_from_slice(&chunk);
  }
  Ok(bytes)
}

#[derive(serde::Deserialize)]
struct GhAsset {
  #[serde(rename = "browser_download_url")]
  download_url: String,
  name: String,
}

#[derive(serde::Deserialize)]
struct GhLatest {
  tag_name: String,
  draft: bool,
  prerelease: bool,
  assets: Vec<GhAsset>,
}

fn find_asset_url(latest: &GhLatest, name: &str) -> Result<String, String> {
  if latest.draft || latest.prerelease {
    return Err("最新 Release 不是公开稳定版".into());
  }
  let mut found: Option<String> = None;
  for asset in &latest.assets {
    if asset.name == name {
      found = Some(asset.download_url.clone());
      break;
    }
  }
  found.ok_or_else(|| format!("最新 Release ({}) 缺少资产 {name}", latest.tag_name))
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
) -> Result<UpdaterStatus, String> {
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
) -> Result<UpdateCheck, String> {
  let mut state = store
    .try_lock()
    .map_err(|_| "更新操作正在进行，请勿重复操作")?;
  state.clear();
  let status = configuration(&app);
  let current = app.package_info().version.to_string();
  if status.configured {
    let config = app.state::<crate::SharedConfig>().load();
    let client = update_client(&config.network)?;
    let api_raw = bounded_download(&client, API_LATEST, 64 * 1024).await?;
    let latest: GhLatest =
      serde_json::from_slice(&api_raw).map_err(|e| format!("无效 GitHub API 响应: {e}"))?;
    let manifest_url = find_asset_url(&latest, "portable-latest.json")?;
    let sig_url = find_asset_url(&latest, "portable-latest.json.sig")?;
    // 额外安全：URL 必须是自有仓库 tag 路径
    let expected_prefix = format!(
      "https://github.com/{OWNER}/{REPO}/releases/download/{}/",
      latest.tag_name
    );
    if !manifest_url.starts_with(&expected_prefix) || !sig_url.starts_with(&expected_prefix) {
      return Err("更新资产 URL 不在可信 tag 路径下".into());
    }
    let raw = bounded_download(&client, &manifest_url, 128 * 1024).await?;
    let sig = bounded_download(&client, &sig_url, 1024).await?;
    let sig = String::from_utf8(sig).map_err(|_| "无效更新签名编码")?;
    let manifest: PortableManifest = verify_manifest(&raw, sig.trim(), UPDATE_PUBLIC_KEY, "0.0.0")?;
    let offered = semver::Version::parse(&manifest.version).map_err(|e| e.to_string())?;
    let current_version = semver::Version::parse(&current).map_err(|e| e.to_string())?;
    if offered.cmp_precedence(&current_version).is_gt() {
      state.update = Some(manifest);
      state.raw_manifest = Some(raw);
      state.signature = Some(sig.trim().to_string());
    }
  }
  Ok(UpdateCheck {
    status,
    available: state.update.is_some(),
    current_version: current,
    available_version: state.update.as_ref().map(|update| update.version.clone()),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn update_requests_use_the_enabled_application_proxy() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
      let (mut stream, _) = listener.accept().await.unwrap();
      let mut request = Vec::new();
      while !request.ends_with(b"\r\n\r\n") {
        let byte = stream.read_u8().await.unwrap();
        request.push(byte);
        assert!(request.len() <= 4096);
      }
      let request = String::from_utf8_lossy(&request);
      assert!(request.starts_with("GET http://updater.invalid/manifest "));
      stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
        .await
        .unwrap();
    });
    let network = NetworkSettings {
      enable_proxy: Some(true),
      proxy: Some(format!("http://{address}")),
      ..Default::default()
    };
    let client = update_client(&network).unwrap();
    let bytes = tokio::time::timeout(
      std::time::Duration::from_secs(5),
      bounded_download(&client, "http://updater.invalid/manifest", 2),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(bytes, b"ok");
    server.await.unwrap();
  }

  #[tokio::test]
  async fn update_requests_use_the_enabled_socks_proxy() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
      let (mut stream, _) = listener.accept().await.unwrap();
      assert_eq!(stream.read_u8().await.unwrap(), 5);
      let methods_count = stream.read_u8().await.unwrap();
      let mut methods = vec![0; methods_count as usize];
      stream.read_exact(&mut methods).await.unwrap();
      assert!(methods.contains(&0));
      stream.write_all(&[5, 0]).await.unwrap();
      let mut header = [0; 4];
      stream.read_exact(&mut header).await.unwrap();
      assert_eq!(header, [5, 1, 0, 3]);
      let host_length = stream.read_u8().await.unwrap();
      let mut host = vec![0; host_length as usize];
      stream.read_exact(&mut host).await.unwrap();
      assert_eq!(host, b"updater.invalid");
      assert_eq!(stream.read_u16().await.unwrap(), 80);
      stream
        .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80])
        .await
        .unwrap();
      let mut request = Vec::new();
      while !request.ends_with(b"\r\n\r\n") {
        request.push(stream.read_u8().await.unwrap());
        assert!(request.len() <= 4096);
      }
      assert!(request.starts_with(b"GET /manifest "));
      stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
        .await
        .unwrap();
    });
    let network = NetworkSettings {
      enable_proxy: Some(true),
      proxy: Some(format!("socks5h://{address}")),
      ..Default::default()
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
      let client = update_client(&network)?;
      bounded_download(&client, "http://updater.invalid/manifest", 2).await
    })
    .await;
    // 即便客户端不支持 SOCKS，失败测试也不遗留等待连接的后台任务。
    if !matches!(&result, Ok(Ok(_))) {
      server.abort();
    }
    assert_eq!(result.unwrap().unwrap(), b"ok");
    server.await.unwrap();
  }

  #[test]
  fn enabled_update_proxy_requires_a_nonempty_address() {
    let network = NetworkSettings {
      enable_proxy: Some(true),
      proxy: Some("  ".into()),
      ..Default::default()
    };
    assert!(update_client(&network).is_err());
  }

  #[test]
  fn disabled_update_proxy_ignores_the_saved_address() {
    let network = NetworkSettings {
      enable_proxy: Some(false),
      proxy: Some("not a proxy URL".into()),
      ..Default::default()
    };
    assert!(update_client(&network).is_ok());
  }

  #[test]
  fn trusted_api_allows_only_own_repo_releases() {
    for url in [
      "https://api.github.com/repos/jianjieyiban/JJYB-YouTube-dl/releases/latest",
      "https://api.github.com/repos/jianjieyiban/JJYB-YouTube-dl/releases/tags/v3.4.0",
    ] {
      assert!(super::trusted_api_request(&url.parse().unwrap()));
    }
    for url in [
      "https://api.github.com/repos/other/repo/releases/latest",
      "http://api.github.com/repos/jianjieyiban/JJYB-YouTube-dl/releases/latest",
      "https://api.github.com/repos/jely2002/youtube-dl-gui/releases/latest",
      "https://api.evil.com/repos/jianjieyiban/JJYB-YouTube-dl/releases/latest",
    ] {
      assert!(!super::trusted_api_request(&url.parse().unwrap()));
    }
  }

  #[test]
  fn update_redirects_allow_only_github_https_assets() {
    for url in [
      "https://github.com/jianjieyiban/JJYB-YouTube-dl",
      "https://release-assets.githubusercontent.com/asset?token=temporary",
      "https://objects.githubusercontent.com/asset",
    ] {
      assert!(super::trusted_update_redirect(&url.parse().unwrap()));
    }
    for url in [
      "http://github.com/asset",
      "https://github.com.evil/asset",
      "https://user@github.com/asset",
      "https://github.com:444/asset",
      "https://127.0.0.1/asset",
      "https://jely2002.github.io/asset",
    ] {
      assert!(!super::trusted_update_redirect(&url.parse().unwrap()));
    }
  }

  #[test]
  fn find_asset_url_rejects_draft_and_missing() {
    let mut latest = GhLatest {
      tag_name: "v3.4.0".into(),
      draft: true,
      prerelease: false,
      assets: vec![],
    };
    assert!(super::find_asset_url(&latest, "portable-latest.json").is_err());
    latest.draft = false;
    latest.assets = vec![GhAsset {
      name: "portable-latest.json".into(),
      download_url: "https://github.com/x".into(),
    }];
    assert_eq!(
      super::find_asset_url(&latest, "portable-latest.json").unwrap(),
      "https://github.com/x"
    );
    assert!(super::find_asset_url(&latest, "missing.json").is_err());
  }

  #[test]
  fn clear_invalidates_downloaded_bytes_and_signature() {
    let mut store = super::UpdateStore {
      bytes: Some(vec![1, 2, 3]),
      raw_manifest: Some(vec![1]),
      signature: Some("invalid".into()),
      ..Default::default()
    };
    store.clear();
    assert!(store.bytes.is_none());
    assert!(store.update.is_none());
    assert!(store.raw_manifest.is_none());
    assert!(store.signature.is_none());
  }
}
