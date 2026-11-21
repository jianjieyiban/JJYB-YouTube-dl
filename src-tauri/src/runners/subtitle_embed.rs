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
  #[ignore = "requires a real ffmpeg binary; run explicitly with --ignored and JJYB_TEST_FFMPEG set"]
  async fn started_ffmpeg_nonzero_exit_keeps_original_media_and_srt() {
    let binary = std::env::var("JJYB_TEST_FFMPEG")
      .expect("Set JJYB_TEST_FFMPEG to a real ffmpeg executable for this acceptance test");
    assert!(
      std::path::Path::new(&binary).is_file(),
      "JJYB_TEST_FFMPEG does not point to an existing file"
    );
    let directory = tempfile::tempdir().unwrap();
    let media = directory.path().join("video.mp4");
    let subtitle = directory.path().join("video.en.srt");
    // Synthesize a genuinely valid MP4 with the same ffmpeg binary.
    let synthesized = std::process::Command::new(&binary)
      .args([
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=0.5:size=128x64:rate=10",
        "-pix_fmt",
        "yuv420p",
        "-y",
      ])
      .arg(&media)
      .status()
      .unwrap();
    assert!(
      synthesized.success(),
      "ffmpeg could not synthesize the test MP4"
    );
    // Binary garbage must make ffmpeg fail to open the subtitle input.
    std::fs::write(&subtitle, [0u8, 159, 146, 150, 0, 1, 2, 255, 254, 253]).unwrap();
    let before_media = std::fs::read(&media).unwrap();
    let before_srt = std::fs::read(&subtitle).unwrap();
    let (_running, mut cancel) = watch::channel(true);
    let result = embed_with_binary(
      std::path::Path::new(&binary),
      &media,
      std::slice::from_ref(&subtitle),
      &mut cancel,
    )
    .await;
    let error = result.expect_err("a corrupt SRT must make the started ffmpeg exit nonzero");
    assert!(
      error.contains("FFmpeg subtitle embedding failed"),
      "unexpected error: {error}"
    );
    assert_eq!(std::fs::read(&media).unwrap(), before_media);
    assert_eq!(std::fs::read(&subtitle).unwrap(), before_srt);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
  }

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
