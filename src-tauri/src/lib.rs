mod binaries;
mod commands;
mod i18n;
mod logging;
mod menu;
mod models;
mod parsers;
mod paths;
pub mod portable_transaction;
pub mod portable_update;
mod runners;
mod scheduling;
mod state;
mod stronghold;
mod tray;
mod window;

use crate::binaries::binaries_manager::BinariesManager;
use crate::binaries::binaries_state::BinariesState;
use crate::commands::*;
use crate::i18n::I18nManager;
use crate::logging::LogStoreState;
use crate::menu::setup_menu;
use crate::paths::PathsManager;
use crate::scheduling::concurrency::DynamicSemaphore;
use crate::scheduling::download_pipeline::{setup_download_dispatcher, DownloadSender};
use crate::scheduling::fetch_pipeline::{setup_fetch_dispatcher, FetchSender};
use crate::state::config::ConfigHandle;
use crate::state::preferences::PreferencesHandle;
use crate::tray::{create_tray, TrayState};
use crate::window::{restore_main_window, setup_close_behaviour, track_main_window};
use sentry::ClientInitGuard;
use std::sync::{Arc, Mutex as StdMutex};
use stronghold::stronghold_state;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tokio::sync::Mutex;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::{fmt, prelude::*};

type SharedConfig = Arc<ConfigHandle>;
type SharedPreferences = Arc<PreferencesHandle>;

#[derive(Clone)]
pub struct DownloadLimiter(pub Arc<DynamicSemaphore>);

#[derive(Clone)]
pub struct FetchLimiter(pub Arc<DynamicSemaphore>);

/// # Panics
///
/// Will panic if an error occurs during tauri setup.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let app = tauri::Builder::default()
    .plugin(tauri_plugin_notification::init())
    .plugin(tauri_plugin_autostart::Builder::new().build())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_store::Builder::new().build())
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_keyring::init())
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .plugin(tauri_plugin_autostart::init(
      MacosLauncher::LaunchAgent,
      Some(vec!["--auto-start"]),
    ))
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
      reopen_window(app)
    }))
    .setup(|app| {
      let handle = app.handle();

      if let Ok(dsn) = std::env::var("JJYB_SENTRY_DSN") {
        if !dsn.trim().is_empty() {
          let sentry_client = sentry::init((
            dsn,
            sentry::ClientOptions {
              traces_sample_rate: 0.05,
              sample_rate: 0.25,
              release: sentry::release_name!(),
              ..sentry::ClientOptions::default()
            },
          ));
          handle.manage::<ClientInitGuard>(sentry_client);
        }
      }

      init_tracing();

      // setup runtime mode detection / path management
      let path_handle = match PathsManager::new(handle).and_then(|paths| {
        paths.prepare_writable_dirs()?;
        Ok(paths)
      }) {
        Ok(paths) => paths,
        Err(error) => {
          #[cfg(windows)]
          {
            use tauri_plugin_dialog::DialogExt;
            let exit_handle = handle.clone();
            handle.dialog().message(format!("启动失败：{error}"))
              .title("根目录不可用").show(move |_| exit_handle.exit(1));
            return Ok(());
          }
          #[cfg(not(windows))]
          return Err(error.into());
        }
      };
      handle.manage(path_handle.clone());

      #[cfg(windows)]
      let legacy = {
        let old = handle.path().app_data_dir()?;
        crate::state::json_handle::legacy_stores_available(path_handle.app_dir(), &old)?
          .then_some(old)
      };
      let startup_handle = handle.clone();
      let initialize = move || -> Result<(), Box<dyn std::error::Error>> {
      let handle = &startup_handle;
      // setup config management
      let config_handle = ConfigHandle::init(handle)?;
      let shared_config = Arc::new(config_handle);
      handle.manage::<SharedConfig>(shared_config);

      let preferences_handle = PreferencesHandle::init(handle)?;
      let shared_preferences = Arc::new(preferences_handle);
      handle.manage::<SharedPreferences>(shared_preferences);

      create_main_window(handle, &path_handle)?;

      // setup i18n management
      handle.manage(I18nManager::new(handle));

      // setup window management
      restore_main_window(handle);
      track_main_window(handle);
      setup_close_behaviour(handle);

      // setup tray
      handle.manage(TrayState {
        tray: StdMutex::new(None),
      });
      create_tray(handle);

      // setup autostart
      init_autostart(handle);

      // manage update store
      handle.manage(Mutex::new(UpdateStore::default()));

      // manage log store
      handle.manage(LogStoreState::new());

      // setup dispatchers
      let cfg_snapshot = handle.state::<SharedConfig>().load();
      let max_concurrency = cfg_snapshot.performance.max_concurrency;
      let download_limiter = Arc::new(DynamicSemaphore::new(max_concurrency));
      let fetch_limiter = Arc::new(DynamicSemaphore::new(max_concurrency));
      handle.manage(DownloadLimiter(download_limiter.clone()));
      handle.manage(FetchLimiter(fetch_limiter.clone()));

      let fetch_dispatcher = setup_fetch_dispatcher(handle, fetch_limiter);
      handle.manage(FetchSender(fetch_dispatcher.sender()));
      let download_dispatcher = setup_download_dispatcher(handle, download_limiter);
      handle.manage(DownloadSender(download_dispatcher.sender()));

      // setup binaries
      handle.manage(BinariesState::default());
      handle.manage(BinariesManager::new(handle));

      // set up stronghold
      let app_path = path_handle.app_dir();
      let stronghold_path = app_path.join("vault.hold");

      handle.manage(stronghold_state::StrongholdState::new(stronghold_path));

      // async init stronghold
      #[cfg(not(all(target_os = "macos", debug_assertions)))]
      {
        let state_ref = handle.state::<stronghold_state::StrongholdState>();
        stronghold_state::init_on_startup(handle, &state_ref);
      }

      // configure app menu
      setup_menu(handle);

      // register shortcuts
      register_shortcuts(handle);
      Ok(())
      };

      #[cfg(windows)]
      if let Some(old) = legacy {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        let root = handle.state::<PathsManager>().app_dir().clone();
        let callback_handle = handle.clone();
        handle.dialog()
          .message("检测到旧 AppData 设置。是否导入普通设置与偏好？\n不会复制 Cookies、保险库、认证引用、代理或提取器参数；旧数据保持不变。\n选择全新配置将使用程序旁的 Downloads（导入时保留显式下载目录）。")
          .title("选择根目录配置")
          .buttons(MessageDialogButtons::YesNoCancelCustom(
            "导入设置".into(), "全新配置".into(), "退出".into()))
          .show_with_result(move |result| {
            let decision = crate::state::json_handle::migration_decision(result);
            let result = match decision {
              Some(import) => (|| -> Result<(), Box<dyn std::error::Error>> {
                if import { crate::state::json_handle::import_legacy_stores(&root, &old)?; }
                initialize()
              })(),
              None => { callback_handle.exit(0); return; }
            };
            if let Err(error) = result {
              let exit_handle = callback_handle.clone();
              callback_handle.dialog().message(format!("启动失败：{error}"))
                .title("根目录配置错误").show(move |_| exit_handle.exit(1));
            }
          });
        return Ok(());
      }
      initialize()?;
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      app_ready,
      media_size,
      media_info,
      media_playlist_expand,
      media_download,
      group_cancel,
      logging_subscribe,
      logging_unsubscribe,
      config_get,
      config_reset,
      config_set,
      preferences_get,
      preferences_reset,
      preferences_set,
      binaries_check,
      binaries_ensure,
      binaries_check_updates,
      binaries_update,
      updater_status,
      updater_check,
      updater_download,
      updater_install,
      stronghold_init,
      stronghold_status,
      stronghold_keys,
      stronghold_get,
      stronghold_set,
      get_platform,
      notify,
    ])
    .build(tauri::generate_context!())
    .expect("error while running tauri application");

  app.run(|_app_handle, _event| {
    #[cfg(target_os = "macos")]
    {
      if let tauri::RunEvent::Reopen {
        has_visible_windows,
        ..
      } = _event
      {
        if !has_visible_windows {
          reopen_window(_app_handle);
        }
      }
    }
  });
}

fn create_main_window(app: &AppHandle, paths: &PathsManager) -> tauri::Result<()> {
  let window_config = app
    .config()
    .app
    .windows
    .first()
    .expect("missing main window config");

  #[cfg(not(windows))]
  let _ = paths;

  let builder = tauri::WebviewWindowBuilder::from_config(app, window_config)?;

  #[cfg(windows)]
  let builder = if paths.is_portable_app() {
    builder.data_directory(paths.app_dir().join("webview"))
  } else {
    builder
  };

  builder.build()?;

  Ok(())
}

pub fn init_tracing() {
  let fmt_layer = fmt::layer().with_filter(
    Targets::new()
      .with_target("tauri_plugin_updater", LevelFilter::OFF)
      .with_target(
        "tao::platform_impl::platform::event_loop::runner",
        LevelFilter::OFF,
      )
      .with_target("h2", LevelFilter::OFF)
      .with_target("hyper_util", LevelFilter::OFF)
      .with_default(tracing_levels()),
  );

  let sentry_layer = sentry::integrations::tracing::layer().with_filter(
    Targets::new()
      .with_target("jjyb::tool_probe", LevelFilter::OFF)
      .with_target("tauri_plugin_updater", LevelFilter::OFF)
      .with_default(tracing_levels()),
  );

  tracing_subscriber::registry()
    .with(fmt_layer)
    .with(sentry_layer)
    .init();
}

pub fn init_autostart(app: &AppHandle) {
  let cfg_handle = app.state::<SharedConfig>();
  let cfg = cfg_handle.load();
  let autostart_manager = app.autolaunch();
  let is_enabled = autostart_manager.is_enabled().unwrap_or(false);

  if cfg.system.auto_start_enabled && !is_enabled {
    let _ = autostart_manager.enable();
  } else if !cfg.system.auto_start_enabled && is_enabled {
    let _ = autostart_manager.disable();
  }
}

pub fn reopen_window(app: &AppHandle) {
  let Some(window) = app.get_webview_window("main") else {
    return;
  };
  let _ = window.show();
  let _ = window.set_focus();
}

#[cfg(debug_assertions)]
const fn tracing_levels() -> LevelFilter {
  LevelFilter::DEBUG
}

#[cfg(not(debug_assertions))]
fn tracing_levels() -> LevelFilter {
  LevelFilter::INFO
}
