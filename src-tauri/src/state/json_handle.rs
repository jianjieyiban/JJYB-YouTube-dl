use crate::paths::PathsManager;
use crate::state::json_state::{json_merge, JsonBackedState};
use arc_swap::ArcSwap;
use serde_json::{Map, Value};
use std::{error::Error, sync::Arc};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_store::{Store, StoreExt};

#[cfg(any(windows, test))]
const LEGACY_STORES: [&str; 2] = ["config.store.json", "preferences.store.json"];

#[cfg(any(windows, test))]
pub(crate) fn legacy_stores_available(
  root: &std::path::Path,
  old: &std::path::Path,
) -> std::io::Result<bool> {
  for file in ["config.store.json", "preferences.store.json", "vault.hold"] {
    if root.join(file).try_exists()? {
      return Ok(false);
    }
  }
  for file in LEGACY_STORES {
    if old.join(file).try_exists()? {
      return Ok(true);
    }
  }
  Ok(false)
}

#[cfg(windows)]
pub(crate) fn migration_decision(result: tauri_plugin_dialog::MessageDialogResult) -> Option<bool> {
  use tauri_plugin_dialog::MessageDialogResult;
  match result {
    MessageDialogResult::Custom(label) if label == "导入设置" => Some(true),
    MessageDialogResult::Custom(label) if label == "全新配置" => Some(false),
    _ => None,
  }
}

#[cfg(any(windows, test))]
fn sanitize_config_import(source: &Value) -> Result<Value, Box<dyn Error>> {
  use crate::state::config_models::{AuthSettings, Config};
  let mut config: Config = serde_json::from_value(
    source
      .get("config")
      .ok_or("旧配置缺少 config 字段")?
      .clone(),
  )?;
  config.auth = AuthSettings::default();
  config.network.proxy = None;
  config.network.enable_proxy = Some(false);
  config.network.extractor_args.clear();
  Ok(serde_json::json!({ "config": config }))
}

#[cfg(any(windows, test))]
pub(crate) fn import_legacy_stores(
  root: &std::path::Path,
  old: &std::path::Path,
) -> Result<(), Box<dyn Error>> {
  use crate::state::preferences_models::Preferences;
  if !legacy_stores_available(root, old)? {
    return Err("根目录已有数据，停止导入；不会覆盖现有文件".into());
  }
  let mut staged = Vec::new();
  for file in LEGACY_STORES {
    let source = old.join(file);
    if !source.try_exists()? {
      continue;
    }
    let value: Value = serde_json::from_slice(&std::fs::read(&source)?)?;
    let value = if file == "config.store.json" {
      sanitize_config_import(&value)?
    } else {
      let mut prefs: Preferences = serde_json::from_value(
        value
          .get("preferences")
          .ok_or("旧偏好缺少 preferences 字段")?
          .clone(),
      )?;
      prefs.recents = Default::default();
      serde_json::json!({ "preferences": prefs })
    };
    let mut temporary = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer(temporary.as_file_mut(), &value)?;
    temporary.as_file().sync_all()?;
    staged.push((temporary, root.join(file)));
  }
  for (temporary, destination) in staged {
    temporary.persist_noclobber(destination)?;
  }
  Ok(())
}

pub struct JsonStoreHandle<T: JsonBackedState> {
  swap: Arc<ArcSwap<T>>,
  store: Arc<Store<Wry>>,
}

impl<T: JsonBackedState> JsonStoreHandle<T> {
  pub fn init(app: &AppHandle<Wry>) -> Result<Self, Box<dyn Error>> {
    let paths_manager = app.state::<PathsManager>();
    let data_root = paths_manager.app_dir();
    let store_path = data_root.join(T::STORE_FILE);
    let store = app.store(store_path)?;

    let raw = store
      .get(T::ROOT_KEY)
      .unwrap_or_else(|| Value::Object(Map::new()));

    let mut initial = T::materialize(&raw)?;
    T::before_initialized(app, &mut initial);

    let to_save = serde_json::to_value(&initial)?;
    store.set(T::ROOT_KEY, to_save);
    store.save()?;

    Ok(Self {
      swap: Arc::new(ArcSwap::from_pointee(initial)),
      store,
    })
  }

  #[cfg(windows)]
  pub fn flush(&self) -> Result<(), tauri_plugin_store::Error> {
    self.store.save()
  }

  #[inline]
  pub fn load(&self) -> Arc<T> {
    self.swap.load_full()
  }

  pub fn apply_patch(&self, patch: &Value, app: &AppHandle<Wry>) -> Result<T, Box<dyn Error>> {
    let mut raw = serde_json::to_value(self.swap.load_full().as_ref())?;

    json_merge(&mut raw, patch);

    let new_value = T::materialize(&raw)?;
    T::on_updated(app, &new_value);

    let to_store = serde_json::to_value(&new_value)?;
    self.store.set(T::ROOT_KEY, to_store);
    self.store.save()?;
    self.swap.store(Arc::new(new_value.clone()));

    Ok(new_value)
  }

  pub fn reset(&self, app: &AppHandle<Wry>) -> Result<T, Box<dyn Error>> {
    let mut default_value = T::default_value();
    T::before_initialized(app, &mut default_value);
    let raw = serde_json::to_value(&default_value)?;

    T::on_updated(app, &default_value);

    self.store.set(T::ROOT_KEY, raw);
    self.store.save()?;
    self.swap.store(Arc::new(default_value.clone()));

    Ok(default_value)
  }
}

#[cfg(test)]
mod migration_tests {
  use super::*;

  #[cfg(windows)]
  #[test]
  fn import_new_and_close_are_distinct_decisions() {
    use tauri_plugin_dialog::MessageDialogResult;
    assert_eq!(
      migration_decision(MessageDialogResult::Custom("导入设置".into())),
      Some(true)
    );
    assert_eq!(
      migration_decision(MessageDialogResult::Custom("全新配置".into())),
      Some(false)
    );
    assert_eq!(migration_decision(MessageDialogResult::Cancel), None);
  }

  #[test]
  fn import_preserves_preferences_and_leaves_cookies_vault_and_source_untouched() {
    let root = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let old = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let preferences =
      br#"{"preferences":{"window":{"width":1234},"paths":{"videoDownloadDir":"D:/chosen"},"recents":{"recent":{"cookieFile":["old/private-cookie.txt"]}}}}"#;
    std::fs::write(old.path().join("preferences.store.json"), preferences).unwrap();
    std::fs::write(old.path().join("cookies.txt"), b"private").unwrap();
    std::fs::write(old.path().join("vault.hold"), b"encrypted").unwrap();
    import_legacy_stores(root.path(), old.path()).unwrap();
    let imported: Value =
      serde_json::from_slice(&std::fs::read(root.path().join("preferences.store.json")).unwrap())
        .unwrap();
    assert_eq!(imported["preferences"]["window"]["width"], 1234);
    assert!(!imported.to_string().contains("private-cookie"));
    assert_eq!(
      imported["preferences"]["paths"]["videoDownloadDir"],
      "D:/chosen"
    );
    assert_eq!(
      std::fs::read(old.path().join("preferences.store.json")).unwrap(),
      preferences
    );
    assert_eq!(
      std::fs::read(old.path().join("cookies.txt")).unwrap(),
      b"private"
    );
    assert_eq!(
      std::fs::read(old.path().join("vault.hold")).unwrap(),
      b"encrypted"
    );
    assert!(!root.path().join("cookies.txt").exists());
    assert!(!root.path().join("vault.hold").exists());
  }

  #[test]
  fn import_preserves_preferences_and_explicit_output_but_not_auth_or_secrets() {
    let source = serde_json::json!({"config": {
      "appearance": {"theme": "dark"},
      "auth": {"cookieFile": "old/cookies.txt", "cookieBrowser": "chrome"},
      "network": {"proxy": "http://user:secret@localhost", "extractorArgs": "token=secret"},
      "output": {"downloadDir": "D:/chosen"}, "password": "secret"
    }, "unknownSecret": "secret"});
    let imported = sanitize_config_import(&source).unwrap();
    assert_eq!(imported["config"]["appearance"]["theme"], "dark");
    assert_eq!(imported["config"]["output"]["downloadDir"], "D:/chosen");
    assert!(imported["config"]["auth"]["cookieFile"].is_null());
    assert_eq!(imported["config"]["auth"]["cookieBrowser"], "none");
    assert!(!imported.to_string().contains("secret"));
  }

  #[test]
  fn root_store_or_vault_prevents_legacy_detection() {
    let root = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let old = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    std::fs::write(old.path().join("config.store.json"), b"{}").unwrap();
    assert!(legacy_stores_available(root.path(), old.path()).unwrap());
    for file in ["config.store.json", "preferences.store.json", "vault.hold"] {
      let guard = tempfile::NamedTempFile::new_in(root.path()).unwrap();
      guard.persist(root.path().join(file)).unwrap();
      assert!(!legacy_stores_available(root.path(), old.path()).unwrap());
    }
  }

  #[test]
  fn import_never_overwrites_existing_root_store() {
    let root = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let old = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    std::fs::write(root.path().join("preferences.store.json"), b"existing").unwrap();
    std::fs::write(
      old.path().join("preferences.store.json"),
      b"{\"preferences\":{}}",
    )
    .unwrap();
    assert!(import_legacy_stores(root.path(), old.path()).is_err());
    assert_eq!(
      std::fs::read(root.path().join("preferences.store.json")).unwrap(),
      b"existing"
    );
  }
}
