// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
