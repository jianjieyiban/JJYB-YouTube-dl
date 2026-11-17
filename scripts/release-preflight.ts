import { readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { signBytes } from './release-signing.ts';

async function main() {
  const { version } = JSON.parse(await readFile('package.json', 'utf8'));
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version) || process.env.REQUESTED_TAG !== `v${version}`) throw new Error('Requested tag must match stable package version');
  const tagCommit = execFileSync('git', ['rev-parse', `refs/tags/v${version}^{commit}`], { encoding: 'utf8' }).trim();
  const head = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  if (head !== tagCommit) throw new Error('Checkout must match release tag');
  const tauri = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8'));
  const lock = JSON.parse(await readFile('package-lock.json', 'utf8'));
  const cargo = await readFile('src-tauri/Cargo.toml', 'utf8');
  const cargoLock = (await readFile('src-tauri/Cargo.lock', 'utf8')).replaceAll('\r\n', '\n');
  if (tauri.version !== version || lock.version !== version || lock.packages[''].version !== version
    || !cargo.includes(`version = "${version}"`)
    || !cargoLock.includes(`name = "jjyb-youtube-dl"\nversion = "${version}"`)) throw new Error('Release versions are not synchronized');
  for (const [file, key] of [
    ['src-tauri/src/portable_transaction.rs', process.env.JJYB_UPDATE_PUBLIC_KEY_HEX],
    ['src-tauri/src/binaries/binaries_manager.rs', process.env.JJYB_TOOLS_PUBLIC_KEY_HEX],
  ]) {
    if (!key || !(await readFile(file!, 'utf8')).includes(`"${key}"`)) throw new Error('Signing public key must match the pinned client trust root');
  }
  for (const kind of ['UPDATE', 'TOOLS']) {
    signBytes(Buffer.from('JJYB release preflight'), process.env[`JJYB_${kind}_PRIVATE_KEY_B64`], process.env[`JJYB_${kind}_PUBLIC_KEY_HEX`]);
  }
  console.log(`Validated v${version}, exact source tag and both Ed25519 key pairs`);
}
main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
