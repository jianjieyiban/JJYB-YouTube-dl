use ferrous_opencc::{config::BuiltinConfig, OpenCC};

use serde_json::Value;
use std::{
  io::Write,
  path::{Path, PathBuf},
};

pub fn simplify_srt(text: &str, converter: &OpenCC) -> Result<String, String> {
  if !text.contains(" --> ") {
    return Err("Subtitle output is not valid SRT".into());
  }
  let mut result = String::new();
  for line in text.split_inclusive('\n') {
    if line.trim().is_empty() || line.trim().parse::<u64>().is_ok() || line.contains(" --> ") {
      result.push_str(line);
      continue;
    }
    let mut body = String::new();
    let mut tag = false;
    for ch in line.chars() {
      if ch == '<' || ch == '{' {
        result.push_str(&converter.convert(&body));
        body.clear();
        tag = true;
      }
      if tag {
        result.push(ch);
      } else {
        body.push(ch);
      }
      if ch == '>' || ch == '}' {
        tag = false;
      }
    }
    result.push_str(&converter.convert(&body));
  }
  Ok(result)
}

#[derive(Debug, PartialEq)]
pub enum SubtitleError {
  NoSubtitles,
  Internal(String),
}

impl From<String> for SubtitleError {
  fn from(message: String) -> Self {
    Self::Internal(message)
  }
}

impl From<&str> for SubtitleError {
  fn from(message: &str) -> Self {
    Self::Internal(message.into())
  }
}

impl std::fmt::Display for SubtitleError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::NoSubtitles => write!(f, "No subtitle tracks were downloaded"),
      Self::Internal(message) => write!(f, "{message}"),
    }
  }
}

pub fn finish_subtitles(
  manifest: &Path,
  simplify: bool,
  required: bool,
) -> Result<Vec<PathBuf>, SubtitleError> {
  let data = std::fs::read_to_string(manifest).map_err(|e| e.to_string())?;
  let converter = OpenCC::from_config(BuiltinConfig::T2s).map_err(|e| e.to_string())?;
  let mut files = Vec::new();
  for line in data.lines().filter(|line| !line.trim().is_empty()) {
    let tracks: Value =
      serde_json::from_str(line).map_err(|e| format!("Invalid subtitle artifact manifest: {e}"))?;
    if let Some(tracks) = tracks.as_object() {
      for (language, track) in tracks {
        let path = track
          .get("filepath")
          .and_then(Value::as_str)
          .ok_or("Subtitle artifact has no filepath")?;
        let path = PathBuf::from(path).with_extension("srt");
        if files.contains(&path) {
          continue;
        }
        let text = std::fs::read_to_string(&path)
          .map_err(|e| format!("Missing subtitle output {}: {e}", path.display()))?;
        if text.trim().is_empty() || !text.contains(" --> ") {
          return Err("Subtitle output is empty or invalid SRT".into());
        }
        let language = language.to_ascii_lowercase();
        if simplify && (language == "zh" || language.starts_with("zh-")) {
          let converted = simplify_srt(&text, &converter)?;
          let mut temporary =
            tempfile::NamedTempFile::new_in(path.parent().ok_or("Subtitle path has no parent")?)
              .map_err(|e| e.to_string())?;
          temporary
            .write_all(converted.as_bytes())
            .map_err(|e| e.to_string())?;
          temporary.as_file().sync_all().map_err(|e| e.to_string())?;
          temporary.persist(&path).map_err(|e| e.to_string())?;
        }
        files.push(path);
      }
    }
  }
  if required && files.is_empty() {
    return Err(SubtitleError::NoSubtitles);
  }
  Ok(files)
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn conversion_preserves_timing_tags_and_line_endings() {
    let converter = OpenCC::from_config(BuiltinConfig::T2s).unwrap();
    assert_eq!(
      simplify_srt(
        "1\r\n00:00:01,000 --> 00:00:02,000\r\n<b title=\"繁體\">繁體字幕</b>\r\n",
        &converter
      )
      .unwrap(),
      "1\r\n00:00:01,000 --> 00:00:02,000\r\n<b title=\"繁體\">繁体字幕</b>\r\n"
    );
  }
  #[test]
  fn tracked_artifact_is_converted_without_touching_other_files() {
    let directory = tempfile::tempdir().unwrap();
    let subtitle = directory.path().join("video.zh-Hant.srt");
    let unrelated = directory.path().join("other.srt");
    let manifest = directory.path().join("manifest.jsonl");
    let original = "1\r\n00:00:01,000 --> 00:00:02,000\r\n<b>繁體字幕</b>\r\n";
    std::fs::write(&subtitle, original).unwrap();
    std::fs::write(&unrelated, original).unwrap();
    std::fs::write(
      &manifest,
      serde_json::json!({"zh-Hant": {"filepath": subtitle}}).to_string(),
    )
    .unwrap();
    assert_eq!(
      finish_subtitles(&manifest, true, true).unwrap(),
      vec![subtitle.clone()]
    );
    assert_eq!(
      std::fs::read_to_string(&subtitle).unwrap(),
      original.replace("繁體", "繁体")
    );
    assert_eq!(std::fs::read_to_string(&unrelated).unwrap(), original);
    std::fs::write(&subtitle, original).unwrap();
    finish_subtitles(&manifest, false, true).unwrap();
    assert_eq!(std::fs::read_to_string(&subtitle).unwrap(), original);
  }
  #[test]
  fn empty_artifacts_are_not_success() {
    let manifest = tempfile::NamedTempFile::new().unwrap();
    assert!(finish_subtitles(manifest.path(), true, true).is_err());
  }
}
