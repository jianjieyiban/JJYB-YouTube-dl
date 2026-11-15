use std::path::{Path, PathBuf};
use tokio::sync::watch;

pub(super) async fn embed_with_binary(binary: &Path, manifest: &Path, subtitles: &[PathBuf], cancel: &mut watch::Receiver<bool>) -> Result<(), String> {
  let data = std::fs::read_to_string(manifest).map_err(|e| e.to_string())?;
  if data.trim().is_empty() { return Err("No media artifact was recorded for subtitle embedding".into()); }
  for line in data.lines() {
    if !*cancel.borrow() { return Err("Subtitle embedding cancelled".into()); }
    let media: String = serde_json::from_str(line).map_err(|e| e.to_string())?;
    let media = PathBuf::from(media);
    let extension = media.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    let codec = match extension.as_str() {
      "mp4" | "mov" | "m4v" => "mov_text",
      "mkv" => "srt",
      _ => return Err(format!("Container .{extension} does not support subtitle embedding; independent SRT files are preserved")),
    };
    let temporary = tempfile::Builder::new().suffix(&format!(".{extension}")).tempfile_in(media.parent().ok_or("Missing media parent directory")?).map_err(|e| e.to_string())?.into_temp_path();
    let mut command = tokio::process::Command::new(binary);
    command.args(["-y", "-nostdin", "-i"]).arg(&media);
    for subtitle in subtitles { command.arg("-i").arg(subtitle); }
    command.args(["-map", "0"]);
    for index in 0..subtitles.len() { command.args(["-map", &format!("{}:0", index + 1)]); }
    command.args(["-c", "copy", "-c:s", codec]).arg(&temporary).kill_on_drop(true);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let status = tokio::select! {
      result = child.wait() => result.map_err(|e| e.to_string())?,
      _ = cancel.changed() => { let _ = child.kill().await; return Err("Subtitle embedding cancelled".into()); }
    };
    if !status.success() { return Err(format!("FFmpeg subtitle embedding failed: {status}")); }
    if !*cancel.borrow() { return Err("Subtitle embedding cancelled".into()); }
    temporary.persist(&media).map_err(|e| e.to_string())?;
  }
  Ok(())
}
