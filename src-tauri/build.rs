fn main() {
  let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
  tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
    .expect("Tauri build failed");
  if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
    // 统一由链接器嵌入清单，让应用与测试都启用 Common Controls v6，避免重复资源。
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
  }
}
