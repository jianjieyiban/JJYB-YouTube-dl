use std::path::{Path, PathBuf};
use tokio::sync::watch;

pub(super) async fn embed_with_binary(
  binary: &Path,
  media: &Path,
  subtitles: &[PathBuf],
  cancel: &mut watch::Receiver<bool>,
) -> Result<(), String> {
  if !*cancel.borrow() {
    return Err("Subtitle embedding cancelled".into());
  }
  let extension = media
    .extension()
    .and_then(|value| value.to_str())
    .unwrap_or("")
    .to_ascii_lowercase();
  let codec = match extension.as_str() {
      "mp4" | "mov" | "m4v" => "mov_text",
      "mkv" => "srt",
      _ => return Err(format!("Container .{extension} does not support subtitle embedding; independent SRT files are preserved")),
    };
  let temporary = tempfile::Builder::new()
    .suffix(&format!(".{extension}"))
    .tempfile_in(media.parent().ok_or("Missing media parent directory")?)
    .map_err(|e| e.to_string())?
    .into_temp_path();
  let mut command = tokio::process::Command::new(binary);
  command.args(["-y", "-nostdin", "-i"]).arg(media);
  for subtitle in subtitles {
    command.arg("-i").arg(subtitle);
  }
  command.args(["-map", "0"]);
  for index in 0..subtitles.len() {
    command.args(["-map", &format!("{}:0", index + 1)]);
  }
  command
    .args(["-c", "copy", "-c:s", codec])
    .arg(&temporary)
    .kill_on_drop(true);
  let mut child = command.spawn().map_err(|e| e.to_string())?;
  let status = tokio::select! {
    result = child.wait() => result.map_err(|e| e.to_string())?,
    _ = cancel.changed() => { let _ = child.kill().await; return Err("Subtitle embedding cancelled".into()); }
  };
  if !status.success() {
    return Err(format!("FFmpeg subtitle embedding failed: {status}"));
  }
  if !*cancel.borrow() {
    return Err("Subtitle embedding cancelled".into());
  }
  temporary.persist(media).map_err(|e| e.to_string())?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn failed_embed_keeps_original_media_and_srt() {
    let directory = tempfile::tempdir().unwrap();
    let media = directory.path().join("video.mp4");
    let subtitle = directory.path().join("video.en.srt");
    std::fs::write(&media, b"original media").unwrap();
    std::fs::write(&subtitle, b"original subtitle").unwrap();
    let (_running, mut cancel) = watch::channel(true);
    let result = embed_with_binary(
      &directory.path().join("missing-ffmpeg"),
      &media,
      std::slice::from_ref(&subtitle),
      &mut cancel,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(std::fs::read(media).unwrap(), b"original media");
    assert_eq!(std::fs::read(subtitle).unwrap(), b"original subtitle");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
  }
}
