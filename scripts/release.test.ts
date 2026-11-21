import test from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPairSync } from 'node:crypto';
import { mkdtemp, writeFile, rm, mkdir, copyFile, readFile } from 'node:fs/promises';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { signBytes, verifyBytes } from './release-signing.ts';
import { makeManifest, verifyManifest } from './portable-manifest.ts';
import { checksum, verifyChecksums } from './release-assets.ts';

function identity() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  return {
    private: privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64'),
    public: Buffer.from(publicKey.export({ format: 'jwk' }).x!, 'base64url').toString('hex'),
  };
}
function executable() {
  const bytes = Buffer.alloc(1024);
  bytes.write('MZ');
  bytes.writeUInt32LE(0x80, 0x3c);
  bytes.write('PE\0\0', 0x80);
  bytes.writeUInt16LE(0x8664, 0x84);
  bytes.writeUInt16LE(0xf0, 0x94);
  bytes.writeUInt16LE(0x22, 0x96);
  bytes.writeUInt16LE(0x20b, 0x98);
  return bytes;
}
const date = '2026-11-17T00:00:00.000Z';

test('production builds embed frontend assets and reject missing custom protocol', async () => {
  const cargo = await readFile(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
  assert.match(cargo, /\[features\][\s\S]*?default\s*=\s*\["custom-protocol"\]/);
  assert.match(cargo, /custom-protocol\s*=\s*\["tauri\/custom-protocol"\]/);
  const main = await readFile(new URL('../src-tauri/src/main.rs', import.meta.url), 'utf8');
  assert.match(main, /#\[cfg\(all\(not\(debug_assertions\), not\(feature = "custom-protocol"\)\)\)\]/);
  assert.match(main, /compile_error!/);
});

test('Rust CI builds both embedded frontends before compilation', async () => {
  const workflow = await readFile(new URL('../.github/workflows/rust-ci.yml', import.meta.url), 'utf8');
  const jobs = workflow.split(/^  (?:lint|unit-tests):/m).slice(1);
  assert.equal(jobs.length, 2);
  for (const job of jobs) {
    const build = job.search(/npm run build\r?\n/);
    const compile = job.search(/cargo (?:clippy|llvm-cov) --manifest-path/);
    assert.ok(build >= 0 && build < compile, 'Rust CI must build the main and isolation assets before compiling');
  }
});

test('portable packaging keeps staging under its output root', async () => {
  const source = await readFile(new URL('./package-portable.ps1', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /\$env:RUNNER_TEMP/);
  assert.match(source, /\$layout = Join-Path \$OutputDirectory/);
});

test('release workflow keeps generated notes under the checkout', async () => {
  const source = await readFile(new URL('../.github/workflows/portable-release.yml', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /RUNNER_TEMP|runner\.temp/);
  assert.match(source, /\.release-work\/jjyb-release-notes\.txt/);
});

test('release verification downloads stay under the checkout', async () => {
  const source = await readFile(new URL('./publish-release.ts', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /tmpdir\(/);
  assert.match(source, /resolve\('\.release-work'\)/);
});

test('portable staging reports rejected relative paths without exposing file contents', async () => {
  const source = await readFile(new URL('./package-portable.ps1', import.meta.url), 'utf8');
  assert.match(source, /\$forbidden\s*\|\s*ForEach-Object/);
  assert.match(source, /GetRelativePath\(\$layout,\s*\$_\.FullName\)/);
  assert.match(source, /forbidden runtime or secret artifacts: \$rejected/);
});

test('portable guard allows only the public certifi bundle and rejects secrets', { skip: process.platform !== 'win32' }, async () => {
  const directory = await mkdtemp(join(tmpdir(), 'jjyb-package-guard-'));
  try {
    const source = await readFile(new URL('./package-portable.ps1', import.meta.url), 'utf8');
    const guard = source.slice(source.indexOf('$forbidden ='), source.indexOf('& "$layout/jjyb-portable/bin/yt-dlp.exe"'));
    const script = join(directory, 'guard.ps1');
    await writeFile(script, `param([string]$layout)\n$ErrorActionPreference = 'Stop'\n${guard}`);
    for (const [name, content, allowed] of [
      ['jjyb-portable/bin/_internal/certifi/cacert.pem', '-----BEGIN CERTIFICATE-----\nYWJj\n-----END CERTIFICATE-----\n', true],
      ['jjyb-portable/bin/_internal/certifi/cacert.pem', '-----BEGIN PRIVATE KEY-----\nYWJj\n-----END PRIVATE KEY-----', false],
      ['jjyb-portable/bin/_internal/certifi/cacert.pem', 'garbage', false],
      ['jjyb-portable/bin/_internal/certifi/cacert.pem', '# Public CA bundle\n-----BEGIN CERTIFICATE-----\nYWJj\n-----END CERTIFICATE-----\n', true],
      ['jjyb-portable/bin/_internal/certifi/cacert.pem', '-----BEGIN CERTIFICATE-----\nYWJj\n-----END CERTIFICATE-----\n-----BEGIN PRIVATE KEY-----\nYWJj\n-----END PRIVATE KEY-----', false],
      ['other/cacert.pem', '-----BEGIN CERTIFICATE-----\nYWJj\n-----END CERTIFICATE-----', false],
      ['cookies.txt', 'secret', false],
      ['private.pkcs8', 'secret', false],
      ['debug.log', 'secret', false],
    ] as const) {
      const layout = await mkdtemp(join(directory, 'layout-'));
      const target = join(layout, name);
      await mkdir(join(target, '..'), { recursive: true });
      await writeFile(target, content);
      const result = spawnSync('pwsh', ['-NoProfile', '-File', script, layout], { encoding: 'utf8' });
      assert.equal(result.status === 0, allowed, `${name}: ${result.stderr}`);
    }
  } finally {
    await rm(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
});

test('normal startup without an update does not apply updater-only path restrictions', async () => {
  const source = await readFile(new URL('../src-tauri/src/portable_transaction.rs', import.meta.url), 'utf8');
  const startup = source.slice(source.lastIndexOf('pub fn startup()'));
  assert.ok(startup.indexOf('if !has_transaction') < startup.lastIndexOf('guard_path(&target)'), 'Ordinary startup is blocked by updater path guards');
  assert.ok(startup.indexOf('if args.first().is_some_and(|s| s == CHILD)') < startup.indexOf('guard_path(&target)'), 'Child handshake must retain its path guard');
});

test('every media submission holds the updater lock until it is queued', async () => {
  for (const command of ['media_info', 'media_playlist_expand', 'media_size', 'media_download']) {
    const source = await readFile(new URL(`../src-tauri/src/commands/media/${command}.rs`, import.meta.url), 'utf8');
    const lock = source.indexOf('.try_lock()');
    const submit = source.search(/(?:FetchBatch::submit|DownloadBatch::submit_with_attempt)/);
    assert.ok(lock >= 0 && lock < submit, `${command} can submit work during update installation`);
    assert.match(source, /let _update_guard = updater/);
  }
});

test('updater uses configured networking without a fixed executable download deadline', async () => {
  const check = await readFile(new URL('../src-tauri/src/commands/updater/updater_check.rs', import.meta.url), 'utf8');
  const download = await readFile(new URL('../src-tauri/src/commands/updater/updater_download.rs', import.meta.url), 'utf8');
  const cargo = await readFile(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
  assert.match(cargo, /reqwest\s*=\s*\{[^\r\n]*features\s*=\s*\[[^\]]*"socks"/);
  const client = check.slice(check.indexOf('pub fn update_client'), check.indexOf('pub async fn bounded_download'));
  assert.match(client, /network\.enable_proxy/);
  assert.match(client, /reqwest::Proxy::all/);
  assert.match(client, /\.read_timeout\(/);
  assert.doesNotMatch(client, /\.timeout\(/);
  for (const source of [check, download]) assert.match(source, /update_client\(&.*network\)/);
  const bounded = check.slice(check.indexOf('pub async fn bounded_download'), check.indexOf('struct GhAsset'));
  assert.match(bounded, /\.timeout\(/);
});

test('update readiness is confirmed only after showing the normal window', async () => {
  const source = await readFile(new URL('../src-tauri/src/commands/app_ready.rs', import.meta.url), 'utf8');
  assert.ok(source.indexOf('.show()') < source.indexOf('confirm_ready()'), 'Update is acknowledged before window display succeeds');
});

test('isolation permits the tool update commands used by the frontend', async () => {
  const isolation = await readFile(new URL('../src-isolation/main.ts', import.meta.url), 'utf8');
  for (const command of ['binaries_check_updates', 'binaries_update']) {
    assert.ok(isolation.includes(`'${command}'`), `Isolation blocks ${command}`);
  }
});

test('release preflight accepts the shared updater trust root and rejects a foreign key', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'jjyb-preflight-test-'));
  try {
    const update = identity();
    const tools = identity();
    for (const path of ['scripts', 'src-tauri/src/commands/updater', 'src-tauri/src/binaries']) await mkdir(join(directory, path), { recursive: true });
    for (const name of ['release-preflight.ts', 'release-signing.ts']) await copyFile(fileURLToPath(new URL(name, import.meta.url)), join(directory, 'scripts', name));
    await writeFile(join(directory, 'package.json'), JSON.stringify({ type: 'module', version: '3.4.0' }));
    await writeFile(join(directory, 'package-lock.json'), JSON.stringify({ version: '3.4.0', packages: { '': { version: '3.4.0' } } }));
    await writeFile(join(directory, 'src-tauri/tauri.conf.json'), JSON.stringify({ version: '3.4.0' }));
    await writeFile(join(directory, 'src-tauri/Cargo.toml'), 'version = "3.4.0"');
    await writeFile(join(directory, 'src-tauri/Cargo.lock'), 'name = "jjyb-youtube-dl"\nversion = "3.4.0"');
    await writeFile(join(directory, 'src-tauri/src/portable_transaction.rs'), `pub const UPDATE_PUBLIC_KEY: &str = "${update.public}";`);
    await writeFile(join(directory, 'src-tauri/src/commands/updater/updater_check.rs'), 'pub use crate::portable_transaction::UPDATE_PUBLIC_KEY;');
    await writeFile(join(directory, 'src-tauri/src/binaries/binaries_manager.rs'), `const TOOLS_PUBLIC_KEY: &str = "${tools.public}";`);
    const git = (...args: string[]) => execFileSync('git', args, { cwd: directory, stdio: 'pipe' });
    git('init');
    git('add', '.');
    git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'fixture');
    git('tag', 'v3.4.0');
    const env = { ...process.env, REQUESTED_TAG: 'v3.4.0', JJYB_UPDATE_PUBLIC_KEY_HEX: update.public, JJYB_UPDATE_PRIVATE_KEY_B64: update.private, JJYB_TOOLS_PUBLIC_KEY_HEX: tools.public, JJYB_TOOLS_PRIVATE_KEY_B64: tools.private };
    const run = () => spawnSync(process.execPath, ['scripts/release-preflight.ts'], { cwd: directory, env, encoding: 'utf8' });
    const valid = run();
    assert.equal(valid.status, 0, valid.stderr);
    env.JJYB_UPDATE_PUBLIC_KEY_HEX = identity().public;
    const foreign = run();
    assert.notEqual(foreign.status, 0);
    assert.match(foreign.stderr, /trust root/);
  } finally {
    await rm(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
});

test('portable manifest signs exact raw bytes and final x64 EXE', () => {
  const key = identity();
  const bytes = executable();
  const raw = Buffer.from(JSON.stringify(makeManifest(bytes, '3.4.0', '更新说明', date), null, 2));
  const signature = signBytes(raw, key.private, key.public);
  const manifest = verifyManifest(raw, signature, key.public, bytes, '3.4.0');
  assert.equal(manifest.product, 'JJYB-YouTube-dl');
  assert.equal(manifest.url, 'https://github.com/jianjieyiban/JJYB-YouTube-dl/releases/download/v3.4.0/JJYB-YouTube-dl_3.4.0_x64.exe');
  assert.throws(() => verifyBytes(Buffer.concat([raw, Buffer.from('\n')]), signature, key.public));
  const changed = Buffer.from(bytes);
  changed[1023] ^= 1;
  assert.throws(() => verifyManifest(raw, signature, key.public, changed, '3.4.0'));
});

test('missing, malformed, foreign and mismatched keys fail closed', () => {
  const key = identity();
  const raw = Buffer.from('manifest');
  for (const privateKey of [undefined, '', 'invalid', 'AAAA']) assert.throws(() => signBytes(raw, privateKey, key.public));
  for (const publicKey of [undefined, '', 'ff', 'z'.repeat(64)]) assert.throws(() => signBytes(raw, key.private, publicKey));
  assert.throws(() => signBytes(raw, key.private, identity().public), /does not match/);
  const signature = signBytes(raw, key.private, key.public);
  assert.throws(() => verifyBytes(raw, signature, identity().public));
  for (const sig of ['', 'AAAA', 'not base64!', `${signature}\n`]) assert.throws(() => verifyBytes(raw, sig, key.public));
});

test('signed manifest rejects wrong identity, channel, platform, URL, hash, version and size', () => {
  const key = identity();
  const bytes = executable();
  const manifest = makeManifest(bytes, '3.4.0', 'notes', date);
  for (const [name, value] of Object.entries({ product: 'other', channel: 'installer', platform: 'windows-aarch64', url: 'https://evil.example/app.exe', sha256: '0'.repeat(64), version: '3.5.0', size: 1025 })) {
    const raw = Buffer.from(JSON.stringify({ ...manifest, [name]: value }));
    assert.throws(() => verifyManifest(raw, signBytes(raw, key.private, key.public), key.public, bytes, '3.4.0'));
  }
  assert.throws(() => verifyManifest(Buffer.from('not json'), 'AAAA', key.public, bytes, '3.4.0'), /signature/i);
});

test('generation rejects unstable versions, missing notes, invalid date and non-x64 executables', () => {
  for (const version of ['3.4.0-beta', 'v3.4.0', '03.4.0', '3.4', '../3.4.0']) assert.throws(() => makeManifest(executable(), version, 'notes', date));
  assert.throws(() => makeManifest(executable(), '3.4.0', '', date));
  assert.throws(() => makeManifest(executable(), '3.4.0', 'notes', 'invalid'));
  assert.throws(() => makeManifest(Buffer.alloc(10), '3.4.0', 'notes', date));
  for (const offset of [0, 0x84, 0x98]) {
    const bytes = executable();
    bytes[offset] = 0;
    assert.throws(() => makeManifest(bytes, '3.4.0', 'notes', date));
  }
});

test('all assets must be present, unique, named safely and hash-correct', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'jjyb-release-test-'));
  try {
    await writeFile(join(directory, 'asset.exe'), 'test');
    const sums = `${checksum(Buffer.from('test'))}  asset.exe\n`;
    await writeFile(join(directory, 'SHA256SUMS'), sums);
    await verifyChecksums(directory, ['asset.exe']);
    await assert.rejects(verifyChecksums(directory, ['missing.exe']));
    await writeFile(join(directory, 'asset.exe'), 'tampered');
    await assert.rejects(verifyChecksums(directory), /SHA256 mismatch/);
    for (const text of [sums + sums, sums.replace('asset.exe', '../asset.exe'), sums.replace('asset.exe', 'SHA256SUMS')]) {
      await writeFile(join(directory, 'SHA256SUMS'), text);
      await assert.rejects(verifyChecksums(directory));
    }
  } finally {
    await rm(directory, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
});
