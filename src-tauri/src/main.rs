// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!("Production builds must enable custom-protocol to embed frontend assets");

fn main() {
  match open_video_downloader_lib::portable_transaction::startup() {
    Ok(true) => return,
    Ok(false) => {}
    Err(error) => {
      eprintln!("Portable update startup failed: {error}");
      std::process::exit(1);
    }
  }
  open_video_downloader_lib::run();
}
