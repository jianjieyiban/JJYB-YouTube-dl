use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MIN_SIZE: u64 = 1024;
const MAX_SIZE: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableManifest {
  pub product: String,
  pub platform: String,
  pub channel: String,
  pub version: String,
  pub url: String,
  pub size: u64,
  pub sha256: String,
  pub notes: String,
  pub pub_date: String,
}

pub fn verify_manifest(
  raw: &[u8],
  signature: &str,
  public_key: &str,
  current_version: &str,
) -> Result<PortableManifest, String> {
  let key_bytes: [u8; 32] = hex::decode(public_key)
    .map_err(|_| "Invalid public key hex")?
    .try_into()
    .map_err(|_| "Public key must be 32 bytes")?;
  let key = VerifyingKey::from_bytes(&key_bytes).map_err(|_| "Invalid public key")?;
  let signature_bytes = STANDARD
    .decode(signature)
    .map_err(|_| "Invalid signature base64")?;
  let signature =
    Signature::from_slice(&signature_bytes).map_err(|_| "Invalid signature length")?;
  key
    .verify_strict(raw, &signature)
    .map_err(|_| "Invalid manifest signature")?;

  let manifest: PortableManifest =
    serde_json::from_slice(raw).map_err(|error| format!("Invalid manifest JSON: {error}"))?;
  if manifest.product != "JJYB-YouTube-dl"
    || manifest.platform != "windows-x86_64"
    || manifest.channel != "portable"
  {
    return Err("Unexpected manifest product, platform or channel".into());
  }
  let version = Version::parse(&manifest.version).map_err(|_| "Invalid manifest version")?;
  let current = Version::parse(current_version).map_err(|_| "Invalid current version")?;
  if !version.pre.is_empty() || version.cmp_precedence(&current).is_le() {
    return Err("Manifest must offer a newer stable version".into());
  }
  let expected_url = format!(
    "https://github.com/jianjieyiban/JJYB-YouTube-dl/releases/download/v{0}/JJYB-YouTube-dl_{0}_x64.exe",
    manifest.version
  );
  if manifest.url != expected_url {
    return Err("Unexpected manifest asset URL".into());
  }
  if !(MIN_SIZE..=MAX_SIZE).contains(&manifest.size) {
    return Err("Manifest executable size is out of bounds".into());
  }
  if manifest.sha256.len() != 64 || !manifest.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
    return Err("Manifest SHA256 must be 64 hex characters".into());
  }
  Ok(manifest)
}

pub fn verify_executable(bytes: &[u8], manifest: &PortableManifest) -> Result<(), String> {
  if !(MIN_SIZE..=MAX_SIZE).contains(&manifest.size) || bytes.len() as u64 != manifest.size {
    return Err("Executable length does not match manifest".into());
  }
  let expected_hash = hex::decode(&manifest.sha256).map_err(|_| "Invalid manifest SHA256")?;
  if Sha256::digest(bytes).as_slice() != expected_hash.as_slice() {
    return Err("Executable SHA256 does not match manifest".into());
  }
  if bytes.get(..2) != Some(b"MZ") {
    return Err("Executable has no DOS header".into());
  }
  let offset_bytes: [u8; 4] = bytes
    .get(0x3c..0x40)
    .ok_or("Truncated DOS header")?
    .try_into()
    .unwrap();
  let offset =
    usize::try_from(u32::from_le_bytes(offset_bytes)).map_err(|_| "Invalid PE header offset")?;
  let header_end = offset.checked_add(24).ok_or("Invalid PE header offset")?;
  let header = bytes.get(offset..header_end).ok_or("Truncated PE header")?;
  if offset < 0x40 || &header[..4] != b"PE\0\0" || header[4..6] != 0x8664u16.to_le_bytes() {
    return Err("Executable is not an x64 PE".into());
  }
  let optional_size = u16::from_le_bytes([header[20], header[21]]) as usize;
  let characteristics = u16::from_le_bytes([header[22], header[23]]);
  let optional_end = header_end
    .checked_add(optional_size)
    .ok_or("Invalid optional header size")?;
  let optional = bytes
    .get(header_end..optional_end)
    .ok_or("Truncated PE optional header")?;
  if optional_size < 112
    || optional.get(..2) != Some(&0x20bu16.to_le_bytes())
    || characteristics & 0x0002 == 0
    || characteristics & 0x2000 != 0
  {
    return Err("Executable is not a PE32+ application".into());
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use base64::{engine::general_purpose::STANDARD, Engine};
  use ed25519_dalek::{Signer, SigningKey};
  use sha2::{Digest, Sha256};

  // Public RFC 8032 test vector, never a release signing identity.
  fn signed(raw: &[u8]) -> (String, String) {
    let seed =
      hex::decode("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60").unwrap();
    let key = SigningKey::from_bytes(&seed.try_into().unwrap());
    (
      STANDARD.encode(key.sign(raw).to_bytes()),
      hex::encode(key.verifying_key().to_bytes()),
    )
  }

  fn executable() -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
    bytes[0x96..0x98].copy_from_slice(&0x22u16.to_le_bytes());
    bytes[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
    bytes
  }

  fn manifest() -> PortableManifest {
    PortableManifest {
      product: "JJYB-YouTube-dl".into(),
      platform: "windows-x86_64".into(),
      channel: "portable".into(),
      version: "3.4.0".into(),
      url: "https://github.com/jianjieyiban/JJYB-YouTube-dl/releases/download/v3.4.0/JJYB-YouTube-dl_3.4.0_x64.exe".into(),
      size: 1024,
      sha256: hex::encode(Sha256::digest(executable())),
      notes: "更新说明".into(),
      pub_date: "2026-11-17T00:00:00Z".into(),
    }
  }

  fn check(m: &PortableManifest, current: &str) -> Result<PortableManifest, String> {
    let raw = serde_json::to_vec(m).unwrap();
    let (sig, key) = signed(&raw);
    verify_manifest(&raw, &sig, &key, current)
  }

  #[test]
  fn accepts_signed_manifest_and_x64_executable() {
    let verified = check(&manifest(), "3.3.1").unwrap();
    assert_eq!(verified.version, "3.4.0");
    verify_executable(&executable(), &verified).unwrap();
  }

  #[test]
  fn rejects_tampering_and_checks_signature_before_json() {
    let raw = serde_json::to_vec(&manifest()).unwrap();
    let (sig, key) = signed(&raw);
    let mut changed = raw.clone();
    changed.push(b' ');
    assert!(verify_manifest(&changed, &sig, &key, "3.3.1").is_err());
    let mut bad_sig = STANDARD.decode(&sig).unwrap();
    bad_sig[0] ^= 1;
    assert!(verify_manifest(&raw, &STANDARD.encode(bad_sig), &key, "3.3.1").is_err());
    let error = verify_manifest(b"not json", &sig, &key, "3.3.1").unwrap_err();
    assert!(error.contains("signature"), "{error}");
    let (sig, key) = signed(b"not json");
    assert!(verify_manifest(b"not json", &sig, &key, "3.3.1")
      .unwrap_err()
      .contains("JSON"));
  }

  #[test]
  fn rejects_invalid_signature_and_key_encodings() {
    let raw = serde_json::to_vec(&manifest()).unwrap();
    let (sig, key) = signed(&raw);
    for bad in ["", "not base64!", "AAAA"] {
      assert!(verify_manifest(&raw, bad, &key, "3.3.1").is_err());
    }
    for bad in ["", "ff", &"z".repeat(64), &"0".repeat(64)] {
      assert!(verify_manifest(&raw, &sig, bad, "3.3.1").is_err());
    }
  }

  #[test]
  fn rejects_downgrade_same_version_and_prerelease() {
    for current in ["3.4.0", "3.5.0", "4.0.0", "invalid"] {
      assert!(check(&manifest(), current).is_err(), "{current}");
    }
    assert!(check(&manifest(), "3.3.10").is_ok());
    for version in ["3.5.0-beta.1", "3.4", "v3.4.0", "03.4.0"] {
      let mut m = manifest();
      m.version = version.into();
      m.url = format!("https://github.com/jianjieyiban/JJYB-YouTube-dl/releases/download/v{version}/JJYB-YouTube-dl_{version}_x64.exe");
      assert!(check(&m, "3.3.1").is_err(), "{version}");
    }
  }

  #[test]
  fn rejects_wrong_identity_and_untrusted_urls() {
    for field in ["product", "platform", "channel"] {
      let mut value = serde_json::to_value(manifest()).unwrap();
      value[field] = "wrong".into();
      let raw = serde_json::to_vec(&value).unwrap();
      let (sig, key) = signed(&raw);
      assert!(verify_manifest(&raw, &sig, &key, "3.3.1").is_err());
    }
    let good = manifest().url;
    for url in [
      good.replace("jianjieyiban", "attacker"),
      good.replace("https:", "http:"),
      good.replace("github.com/", "github.com.evil/"),
      good.replace("v3.4.0/", "v3.3.1/"),
      format!("{good}?download=1"),
      format!("{good}#fragment"),
      good.replace("github.com/", "user@github.com/"),
    ] {
      let mut m = manifest();
      m.url = url;
      assert!(check(&m, "3.3.1").is_err());
    }
  }

  #[test]
  fn enforces_manifest_size_hash_and_required_strings() {
    for size in [0, 1023, 512 * 1024 * 1024 + 1, u64::MAX] {
      let mut m = manifest();
      m.size = size;
      assert!(check(&m, "3.3.1").is_err());
    }
    let mut m = manifest();
    m.size = 512 * 1024 * 1024;
    assert!(check(&m, "3.3.1").is_ok());
    for hash in ["a".repeat(63), "a".repeat(65), "g".repeat(64)] {
      m.sha256 = hash;
      assert!(check(&m, "3.3.1").is_err());
    }
    m.sha256 = manifest().sha256.to_uppercase();
    assert!(check(&m, "3.3.1").is_ok());
    for field in ["notes", "pub_date"] {
      let mut value = serde_json::to_value(manifest()).unwrap();
      value[field] = 42.into();
      let raw = serde_json::to_vec(&value).unwrap();
      let (sig, key) = signed(&raw);
      assert!(verify_manifest(&raw, &sig, &key, "3.3.1").is_err());
    }
  }

  #[test]
  fn rejects_executable_length_and_hash_mismatch() {
    let m = manifest();
    assert!(verify_executable(&executable()[..1023], &m).is_err());
    let mut bytes = executable();
    bytes[1000] ^= 1;
    assert!(verify_executable(&bytes, &m).is_err());
    let mut upper = m.clone();
    upper.sha256 = upper.sha256.to_uppercase();
    assert!(verify_executable(&executable(), &upper).is_ok());
  }

  #[test]
  fn rejects_fake_truncated_or_non_x64_pe_even_with_matching_hash() {
    for (offset, replacement) in [
      (0, vec![0, 0]),
      (0x3c, u32::MAX.to_le_bytes().to_vec()),
      (0x3c, 0u32.to_le_bytes().to_vec()),
      (0x3c, 1010u32.to_le_bytes().to_vec()),
      (0x80, b"PX\0\0".to_vec()),
      (0x84, 0x14cu16.to_le_bytes().to_vec()),
      (0x98, 0x10bu16.to_le_bytes().to_vec()),
      (0x94, 0u16.to_le_bytes().to_vec()),
      (0x96, 0x2022u16.to_le_bytes().to_vec()),
    ] {
      let mut bytes = executable();
      bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
      let mut m = manifest();
      m.sha256 = hex::encode(Sha256::digest(&bytes));
      assert!(verify_executable(&bytes, &m).is_err(), "offset {offset}");
    }
  }
}
